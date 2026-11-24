#![windows_subsystem = "windows"]

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, UdpSocket};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Color32, RichText, Stroke, Vec2};
use phone_mirror_adb::{
    check_phone_stopped_blocking, notify_pc_stopped, parse_companion_packet, request_stream_authorization_blocking,
    AdbClient, AdbDevice, PC_NOTIFY_PORT,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const NO_WINDOW: u32 = 0x0800_0000;

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Wifi,
    Usb,
}

#[derive(PartialEq, Clone, Copy)]
enum Quality {
    Fps60,
    Fps120,
}

enum Msg {
    Devices(Vec<AdbDevice>),
    CompanionFound(AdbDevice),
    PhoneStoppedStream,
    Ready(String),
    Failed(String),
}

struct App {
    mode: Mode,
    devices: Vec<AdbDevice>,
    selected: Option<String>,
    quality: Quality,
    screen_off: bool,
    scanning: bool,

    busy: Option<String>,
    message: Option<(String, bool)>,

    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    adb: AdbClient,
    child: Option<Child>,
    child_log: Arc<Mutex<Vec<String>>>,
    last_refresh: Instant,
    ctx: egui::Context,
    _tray: Option<TrayIcon>,
}

const TRAY_OPEN_ID: &str = "pm_tray_open";
const TRAY_QUIT_ID: &str = "pm_tray_quit";

// HWND da janela principal e PID do scrcpy, acessados pelos handlers da bandeja.
// Os handlers rodam mesmo com a janela oculta (quando o eframe não chama update()).
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static CHILD_PID: AtomicU32 = AtomicU32::new(0);

fn show_main_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    let h = MAIN_HWND.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            ShowWindow(h as _, SW_SHOW);
            ShowWindow(h as _, SW_RESTORE);
            SetForegroundWindow(h as _);
        }
    }
}

fn hide_main_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    let h = MAIN_HWND.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            ShowWindow(h as _, SW_HIDE);
        }
    }
}

fn kill_child_by_pid() {
    let pid = CHILD_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(NO_WINDOW)
            .status();
    }
}

fn install_tray_handlers(ctx: egui::Context) {
    let ctx_menu = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == TRAY_OPEN_ID {
            show_main_window();
            ctx_menu.request_repaint();
        } else if event.id == TRAY_QUIT_ID {
            kill_child_by_pid();
            std::process::exit(0);
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
        | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
            show_main_window();
            ctx.request_repaint();
        }
        _ => {}
    }));
}

fn create_tray_icon() -> Option<TrayIcon> {
    let menu = Menu::new();
    let item_open = MenuItem::with_id(TRAY_OPEN_ID, "Abrir PhoneMirror", true, None);
    let item_quit = MenuItem::with_id(TRAY_QUIT_ID, "Fechar PhoneMirror", true, None);
    let _ = menu.append(&item_open);
    let _ = menu.append(&item_quit);

    // Cria um ícone 16x16 azul padrão RGBA para a bandeja do sistema
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            if (2..14).contains(&x) && (1..15).contains(&y) {
                if x == 2 || x == 13 || y == 1 || y == 14 {
                    rgba.extend_from_slice(&[10, 132, 255, 255]); // Borda azul iOS
                } else if y >= 11 && (7..9).contains(&x) {
                    rgba.extend_from_slice(&[255, 255, 255, 255]); // Botão home branco
                } else {
                    rgba.extend_from_slice(&[20, 20, 24, 255]); // Tela escura
                }
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]); // Transparente
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, 16, 16).ok().and_then(|icon| {
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("PhoneMirror - Clique para abrir")
            .with_icon(icon)
            .build()
            .ok()
    })
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let ctx = cc.egui_ctx.clone();

        // Thread dedicada para escutar anúncios em tempo real do app Android (porta 45455)
        let tx_listen = tx.clone();
        let ctx_listen = ctx.clone();
        std::thread::spawn(move || {
            if let Ok(sock) = UdpSocket::bind(("0.0.0.0", PC_NOTIFY_PORT)) {
                let mut buf = [0u8; 512];
                while let Ok((len, addr)) = sock.recv_from(&mut buf) {
                    if let SocketAddr::V4(src) = addr {
                        if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                            let trimmed = text.trim();
                            if trimmed.starts_with("PHONEMIRROR_STOP") {
                                let _ = tx_listen.send(Msg::PhoneStoppedStream);
                                ctx_listen.request_repaint();
                            } else if let Some(dev) = parse_companion_packet(trimmed, *src.ip()) {
                                let _ = tx_listen.send(Msg::CompanionFound(dev));
                                ctx_listen.request_repaint();
                            }
                        }
                    }
                }
            }
        });

        // Guarda o HWND da janela para os handlers da bandeja
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(w) = handle.as_raw() {
                MAIN_HWND.store(w.hwnd.get(), Ordering::SeqCst);
            }
        }
        install_tray_handlers(ctx.clone());
        let tray = create_tray_icon();

        let mut app = Self {
            mode: Mode::Wifi,
            devices: Vec::new(),
            selected: None,
            quality: Quality::Fps120,
            screen_off: false,
            scanning: false,
            busy: None,
            message: None,
            tx,
            rx,
            adb: AdbClient::new().unwrap_or_else(|_| AdbClient::new().unwrap()),
            child: None,
            child_log: Arc::new(Mutex::new(Vec::new())),
            last_refresh: Instant::now() - Duration::from_secs(60),
            ctx,
            _tray: tray,
        };
        app.refresh_devices();
        app
    }

    /// Roda uma tarefa async em thread separada e acorda a UI ao terminar.
    fn spawn<F, Fut>(&self, f: F)
    where
        F: FnOnce(AdbClient, Sender<Msg>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()>,
    {
        let adb = self.adb.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(f(adb, tx));
            }
            ctx.request_repaint();
        });
    }

    fn refresh_devices(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.last_refresh = Instant::now();
        self.spawn(|adb, tx| async move {
            let (mut adb_list, companions) = tokio::join!(
                async { adb.list_devices().await.unwrap_or_default() },
                adb.scan_wifi_companions()
            );

            // Combina aparelhos descobertos com o aplicativo PhoneMirror na rede Wi-Fi
            for comp in companions {
                if let Some(existing) = adb_list.iter_mut().find(|d| d.serial == comp.serial) {
                    if comp.model.is_some() {
                        existing.model = comp.model;
                    }
                } else {
                    adb_list.push(comp);
                }
            }

            let _ = tx.send(Msg::Devices(adb_list));
        });
    }

    fn connect_device(&mut self, serial: String) {
        if self.busy.is_some() || self.child.is_some() {
            return;
        }
        self.selected = Some(serial.clone());
        self.message = None;

        let dev_name = self
            .devices
            .iter()
            .find(|d| d.serial == serial)
            .and_then(|d| d.model.clone())
            .unwrap_or_else(|| serial.clone());

        let usb_helper_serial = self
            .devices
            .iter()
            .find(|d| !d.is_wireless && d.state == "device")
            .map(|d| d.serial.clone());

        if let Some((ip, port_str)) = serial.split_once(':') {
            let ip = ip.to_string();
            let port: u16 = port_str.parse().unwrap_or(5555);
            self.busy = Some(format!("Aceite a transmissão na tela de {}...", dev_name));
            self.spawn(move |adb, tx| async move {
                let ip_auth = ip.clone();
                match tokio::task::spawn_blocking(move || request_stream_authorization_blocking(&ip_auth)).await {
                    Ok(Ok(true)) => {}
                    Ok(Ok(false)) => {
                        let _ = tx.send(Msg::Failed(format!("Transmissão recusada em {}.", dev_name)));
                        return;
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(Msg::Failed(e));
                        return;
                    }
                    Err(_) => {
                        let _ = tx.send(Msg::Failed("Falha ao aguardar autorização do celular.".into()));
                        return;
                    }
                }

                if let Some(usb_s) = usb_helper_serial {
                    let _ = adb.enable_tcpip(Some(&usb_s), port).await;
                }
                match adb.connect_tcp(&ip, port).await {
                    Ok(_) => {
                        let _ = tx.send(Msg::Ready(format!("{}:{}", ip, port)));
                    }
                    Err(_) => {
                        let _ = tx.send(Msg::Failed(format!(
                            "Não foi possível conectar a {}. Ative a Depuração Wi-Fi (porta 5555) nas Opções do desenvolvedor.",
                            dev_name
                        )));
                    }
                }
            });
        } else {
            self.busy = Some(format!("Abrindo a tela de {}...", dev_name));
            let _ = self.tx.send(Msg::Ready(serial));
        }
    }

    fn connect(&mut self) {
        if self.busy.is_some() || self.child.is_some() {
            return;
        }
        self.message = None;

        match self.mode {
            Mode::Wifi => {
                let Some(serial) = self.selected.clone() else {
                    self.message = Some((
                        "Nenhum celular selecionado. Abra o app PhoneMirror no celular e clique em Varredura.".into(),
                        true,
                    ));
                    return;
                };
                self.connect_device(serial);
            }
            Mode::Usb => {
                let usb = self.devices.iter().find(|d| !d.is_wireless && d.state == "device");
                match usb {
                    Some(d) => {
                        self.connect_device(d.serial.clone());
                    }
                    None => {
                        self.message = Some((
                            "Nenhum celular no cabo. Ligue a Depuração USB e autorize no celular.".into(),
                            true,
                        ));
                    }
                }
            }
        }
    }

    fn launch(&mut self, serial: &str) {
        let mut cmd = Command::new(find_scrcpy());
        cmd.env("ADB", self.adb.get_adb_path())
            .arg(format!("--serial={}", serial))
            .arg(match self.quality {
                Quality::Fps60 => "--max-fps=60",
                Quality::Fps120 => "--max-fps=120",
            })
            .arg(match self.quality {
                Quality::Fps60 => "--video-bit-rate=8M",
                Quality::Fps120 => "--video-bit-rate=16M",
            })
            .args(["--max-size=1920", "--video-codec=h264", "--stay-awake", "--window-title=PhoneMirror"])
            .args(["--keyboard=uhid", "--gamepad=uhid"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(NO_WINDOW);
        if self.screen_off {
            cmd.arg("--turn-screen-off");
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let log = self.child_log.clone();
                log.lock().unwrap().clear();
                if let Some(err) = child.stderr.take() {
                    std::thread::spawn(move || {
                        for line in BufReader::new(err).lines().map_while(Result::ok) {
                            log.lock().unwrap().push(line);
                        }
                    });
                }
                CHILD_PID.store(child.id(), Ordering::SeqCst);
                self.child = Some(child);
                self.message = Some(("Tela do celular aberta.".into(), false));
            }
            Err(_) => {
                self.message = Some(("Componente de vídeo (scrcpy) não encontrado. Instale com: winget install Genymobile.scrcpy".into(), true));
            }
        }
    }

    fn stop(&mut self) {
        CHILD_PID.store(0, Ordering::SeqCst);
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            self.message = Some(("Desconectado.".into(), false));
        }
        if let Some(sel) = &self.selected {
            if let Some((ip, _)) = sel.split_once(':') {
                let ip = ip.to_string();
                std::thread::spawn(move || notify_pc_stopped(&ip));
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Devices(list) => {
                    self.scanning = false;
                    self.devices = dedupe(list);
                    let available: Vec<_> = self
                        .devices
                        .iter()
                        .filter(|d| d.state == "device" || d.state == "companion")
                        .collect();
                    if !available.iter().any(|d| Some(&d.serial) == self.selected.as_ref()) {
                        self.selected = available.first().map(|d| d.serial.clone());
                    }
                }
                Msg::CompanionFound(comp) => {
                    if let Some(existing) = self.devices.iter_mut().find(|d| d.serial == comp.serial) {
                        if comp.model.is_some() {
                            existing.model = comp.model;
                        }
                    } else {
                        if self.selected.is_none() {
                            self.selected = Some(comp.serial.clone());
                        }
                        self.devices.push(comp);
                        self.devices = dedupe(std::mem::take(&mut self.devices));
                    }
                }
                Msg::PhoneStoppedStream => {
                    CHILD_PID.store(0, Ordering::SeqCst);
                    if let Some(mut c) = self.child.take() {
                        let _ = c.kill();
                    }
                    self.busy = None;
                    self.message = Some(("Transmissão desativada pelo celular.".into(), false));
                }
                Msg::Ready(serial) => {
                    self.busy = None;
                    self.launch(&serial);
                    self.refresh_devices();
                }
                Msg::Failed(e) => {
                    self.busy = None;
                    self.message = Some((e, true));
                }
            }
        }

        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                CHILD_PID.store(0, Ordering::SeqCst);
                self.child = None;
                if let Some(sel) = &self.selected {
                    if let Some((ip, _)) = sel.split_once(':') {
                        let ip = ip.to_string();
                        std::thread::spawn(move || notify_pc_stopped(&ip));
                    }
                }
                if status.success() {
                    self.message = Some(("Sessão encerrada.".into(), false));
                } else {
                    let log = self.child_log.lock().unwrap().clone();
                    self.message = Some((friendly_error(&log), true));
                }
                self.refresh_devices();
            }
        }

        if self.child.is_none() && self.busy.is_none() && self.last_refresh.elapsed() > Duration::from_secs(3) {
            self.refresh_devices();
        } else if self.child.is_some() && self.last_refresh.elapsed() > Duration::from_millis(1200) {
            self.last_refresh = Instant::now();
            if let Some(sel) = &self.selected {
                if let Some((ip, _)) = sel.split_once(':') {
                    let ip = ip.to_string();
                    let tx = self.tx.clone();
                    let ctx = self.ctx.clone();
                    std::thread::spawn(move || {
                        if check_phone_stopped_blocking(&ip) {
                            let _ = tx.send(Msg::PhoneStoppedStream);
                            ctx.request_repaint();
                        }
                    });
                }
            }
        }
    }
}

/// Remove duplicatas do mesmo aparelho (Depuração Wi-Fi gera serial mDNS + IP).
fn dedupe(list: Vec<AdbDevice>) -> Vec<AdbDevice> {
    let mut out: Vec<AdbDevice> = Vec::new();
    for d in list {
        let key = d.model.clone().unwrap_or_else(|| d.serial.clone());
        if let Some(existing) = out.iter_mut().find(|e| e.model.clone().unwrap_or_else(|| e.serial.clone()) == key) {
            // Prefere o serial IP:porta, que funciona em qualquer rede.
            if d.serial.contains(':') && (!existing.serial.contains(':') || d.state == "device") {
                *existing = d;
            }
        } else {
            out.push(d);
        }
    }
    out
}

fn friendly_error(log: &[String]) -> String {
    let all = log.join("\n");
    if all.contains("unauthorized") {
        "Autorize a depuração na tela do celular e tente de novo.".into()
    } else if all.contains("Could not connect") || all.contains("Could not find any ADB device") || all.contains("not found") {
        "Celular não encontrado. Confira se ele está no mesmo Wi-Fi e com a Depuração ligada.".into()
    } else if let Some(e) = log.iter().rev().find(|l| l.contains("ERROR")) {
        e.replace("ERROR: ", "Erro: ")
    } else {
        "A conexão com o celular foi interrompida.".into()
    }
}

fn find_scrcpy() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let pkgs = PathBuf::from(local).join(r"Microsoft\WinGet\Packages");
        if let Ok(entries) = std::fs::read_dir(&pkgs) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("Genymobile.scrcpy") {
                    if let Ok(sub) = std::fs::read_dir(e.path()) {
                        for s in sub.flatten() {
                            let exe = s.path().join("scrcpy.exe");
                            if exe.exists() {
                                return exe;
                            }
                        }
                    }
                }
            }
        }
    }
    PathBuf::from("scrcpy.exe")
}

// ---------- UI ----------

const BG: Color32 = Color32::from_rgb(28, 28, 30);
const CARD: Color32 = Color32::from_rgb(44, 44, 46);
const BORDER: Color32 = Color32::from_rgb(58, 58, 60);
const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
const RED: Color32 = Color32::from_rgb(255, 69, 58);
const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
const TEXT: Color32 = Color32::from_rgb(245, 245, 247);
const MUTED: Color32 = Color32::from_rgb(152, 152, 157);

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            add(ui);
        });
}

fn row(ui: &mut egui::Ui, label: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(14.0).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

fn centered_btn(
    ui: &mut egui::Ui,
    label: &str,
    size: Vec2,
    fill: Color32,
    text_color: Color32,
    font_size: f32,
    radius: f32,
    enabled: bool,
) -> bool {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let bg = if !enabled {
            fill.gamma_multiply(0.5)
        } else if resp.is_pointer_button_down_on() {
            fill.gamma_multiply(0.85)
        } else if resp.hovered() {
            fill.gamma_multiply(1.08)
        } else {
            fill
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, radius, bg);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::new(font_size, egui::FontFamily::Proportional),
            text_color,
        );
    }
    enabled && resp.clicked()
}



impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(500));

        // Clicar no 'X' esconde a janela na bandeja em vez de encerrar o app
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            hide_main_window();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(22)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);

                // Cabeçalho
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PhoneMirror").size(24.0).color(TEXT).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (c, t) = if self.child.is_some() {
                            (GREEN, "Espelhando")
                        } else if self.busy.is_some() {
                            (ORANGE, "Conectando")
                        } else if self.selected.is_some() {
                            (GREEN, "Celular pronto")
                        } else {
                            (MUTED, "Sem celular")
                        };
                        ui.label(RichText::new(t).size(12.0).color(MUTED));
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(9.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.2, c);
                    });
                });

                // Seletor segmentado (texto 100% centralizado)
                egui::Frame::new()
                    .fill(Color32::from_rgb(36, 36, 38))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        ui.columns(2, |cols| {
                            for (i, (m, label)) in [(Mode::Wifi, "Wi-Fi"), (Mode::Usb, "Cabo USB")]
                                .into_iter()
                                .enumerate()
                            {
                                let on = self.mode == m;
                                let w = cols[i].available_width();
                                if centered_btn(
                                    &mut cols[i],
                                    label,
                                    Vec2::new(w, 32.0),
                                    if on { BLUE } else { Color32::TRANSPARENT },
                                    if on { Color32::WHITE } else { MUTED },
                                    13.5,
                                    8.0,
                                    true,
                                ) {
                                    self.mode = m;
                                    self.message = None;
                                }
                            }
                        });
                    });

                // Conteúdo do modo
                match self.mode {
                    Mode::Wifi => {
                        // Barra superior da seção Wi-Fi com título e botão de varredura
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Aparelhos no Wi-Fi").size(14.0).strong().color(TEXT));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let scan_btn = egui::Button::new(
                                    RichText::new(if self.scanning { "Varrendo..." } else { "🔍 Varredura" })
                                        .size(12.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(58, 58, 62))
                                .stroke(Stroke::NONE)
                                .corner_radius(8.0);
                                if ui.add_enabled(!self.scanning, scan_btn).clicked() {
                                    self.refresh_devices();
                                }
                                if self.scanning {
                                    ui.spinner();
                                }
                            });
                        });

                        let available: Vec<AdbDevice> = self
                            .devices
                            .iter()
                            .filter(|d| d.state == "device" || d.state == "companion")
                            .cloned()
                            .collect();

                        if available.is_empty() {
                            card(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(if self.scanning {
                                            "Procurando aparelhos no Wi-Fi..."
                                        } else {
                                            "Nenhum aparelho detectado ainda"
                                        })
                                        .size(14.0)
                                        .color(MUTED),
                                    );
                                    ui.label(
                                        RichText::new("Abra o app PhoneMirror no celular e toque em Varredura.")
                                            .size(12.0)
                                            .color(MUTED),
                                    );
                                    ui.add_space(4.0);
                                });
                            });
                        } else {
                            // Renderiza um WIDGET/CARD SEPARADO para cada aparelho detectado
                            let mut connect_target: Option<String> = None;

                            for d in &available {
                                let name = d
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| "Android".into())
                                    .replace('_', " ");
                                let serial = d.serial.clone();
                                let is_current = self.selected.as_deref() == Some(&d.serial);
                                let is_connected = self.child.is_some() && is_current;

                                egui::Frame::new()
                                    .fill(CARD)
                                    .stroke(Stroke::new(
                                        if is_current { 1.5_f32 } else { 1.0_f32 },
                                        if is_current { BLUE } else { BORDER },
                                    ))
                                    .corner_radius(12.0)
                                    .inner_margin(egui::Margin::symmetric(16, 12))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            // Ícone e dados do aparelho
                                            ui.label(RichText::new("📱").size(22.0));
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new(&name).size(15.0).strong().color(TEXT));
                                                let subtext = if d.state == "companion" {
                                                    "PhoneMirror App • Wi-Fi"
                                                } else if d.is_wireless {
                                                    "Depuração Wi-Fi Pronta"
                                                } else {
                                                    "Conectado via Cabo"
                                                };
                                                ui.label(RichText::new(subtext).size(12.0).color(GREEN));
                                            });

                                            // Botão individual de conexão no próprio widget do aparelho
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if is_connected {
                                                    if centered_btn(
                                                        ui,
                                                        "Desconectar",
                                                        Vec2::new(104.0, 34.0),
                                                        RED,
                                                        Color32::WHITE,
                                                        13.5,
                                                        8.0,
                                                        true,
                                                    ) {
                                                        self.stop();
                                                    }
                                                } else {
                                                    let is_this_busy = self.busy.is_some() && is_current;
                                                    let btn_label = if is_this_busy { "Conectando..." } else { "Conectar" };
                                                    if centered_btn(
                                                        ui,
                                                        btn_label,
                                                        Vec2::new(98.0, 34.0),
                                                        BLUE,
                                                        Color32::WHITE,
                                                        13.5,
                                                        8.0,
                                                        self.busy.is_none(),
                                                    ) {
                                                        connect_target = Some(serial);
                                                    }
                                                }
                                            });
                                        });
                                    });
                            }

                            if let Some(target) = connect_target {
                                self.connect_device(target);
                            }
                        }

                        if self.devices.iter().any(|d| d.state == "unauthorized") {
                            ui.label(
                                RichText::new("Toque em \"Permitir\" na tela do celular para autorizar a conexão.")
                                    .size(12.0)
                                    .color(ORANGE),
                            );
                        }
                    }
                    Mode::Usb => {
                        let usb = self.devices.iter().find(|d| !d.is_wireless);
                        match usb {
                            Some(d) => {
                                let name = d
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| "Android".into())
                                    .replace('_', " ");
                                let is_authorized = d.state == "device";
                                let is_connected = self.child.is_some() && self.selected.as_deref() == Some(&d.serial);
                                let serial = d.serial.clone();

                                egui::Frame::new()
                                    .fill(CARD)
                                    .stroke(Stroke::new(1.0_f32, BORDER))
                                    .corner_radius(12.0)
                                    .inner_margin(egui::Margin::symmetric(16, 12))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("🔌").size(22.0));
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new(&name).size(15.0).strong().color(TEXT));
                                                let (sub, color) = if is_authorized {
                                                    ("Cabo USB Pronto", GREEN)
                                                } else {
                                                    ("Autorize no celular", ORANGE)
                                                };
                                                ui.label(RichText::new(sub).size(12.0).color(color));
                                            });

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if is_connected {
                                                    if centered_btn(
                                                        ui,
                                                        "Desconectar",
                                                        Vec2::new(104.0, 34.0),
                                                        RED,
                                                        Color32::WHITE,
                                                        13.5,
                                                        8.0,
                                                        true,
                                                    ) {
                                                        self.stop();
                                                    }
                                                } else if centered_btn(
                                                    ui,
                                                    "Conectar",
                                                    Vec2::new(98.0, 34.0),
                                                    BLUE,
                                                    Color32::WHITE,
                                                    13.5,
                                                    8.0,
                                                    is_authorized && self.busy.is_none(),
                                                ) {
                                                    self.connect_device(serial);
                                                }
                                            });
                                        });
                                    });
                            }
                            None => {
                                card(ui, |ui| {
                                    row(ui, "Nenhum celular no cabo USB", |ui| {
                                        ui.label(RichText::new("Conecte via cabo").size(13.0).color(MUTED));
                                    });
                                });
                            }
                        }
                        ui.label(
                            RichText::new("Ligue a Depuração USB nas Opções do desenvolvedor e toque em \"Permitir\" no celular.")
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                }

                // Preferências
                card(ui, |ui| {
                    row(ui, "Fluidez", |ui| {
                        ui.selectable_value(&mut self.quality, Quality::Fps120, "120 FPS");
                        ui.selectable_value(&mut self.quality, Quality::Fps60, "60 FPS");
                    });
                    ui.separator();
                    row(ui, "Apagar tela do celular", |ui| {
                        ui.checkbox(&mut self.screen_off, "");
                    });
                });

                // Status
                if let Some(b) = &self.busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(b).size(13.0).color(MUTED));
                    });
                } else if let Some((m, err)) = &self.message {
                    ui.label(RichText::new(m).size(13.0).color(if *err { RED } else { GREEN }));
                }

                // Botão desconectar global (quando uma sessão de espelhamento estiver ativa)
                if self.child.is_some() {
                    let full_w = ui.available_width();
                    if centered_btn(
                        ui,
                        "Desconectar Sessão",
                        Vec2::new(full_w, 44.0),
                        RED,
                        Color32::WHITE,
                        16.0,
                        12.0,
                        true,
                    ) {
                        self.stop();
                    }
                }

                // Rodapé de Créditos (100% centralizado)
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    let total_w = ui.available_width();
                    let links_w = 324.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(total_w, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add_space(((total_w - links_w) * 0.5).max(0.0));
                            ui.hyperlink_to(
                                RichText::new("🐙 GitHub (@Rafael-Franchini)").size(12.0).color(BLUE),
                                "https://github.com/Rafael-Franchini",
                            );
                            ui.label(RichText::new("•").size(12.0).color(MUTED));
                            ui.hyperlink_to(
                                RichText::new("💼 LinkedIn (Rafael Franchini)").size(12.0).color(BLUE),
                                "https://www.linkedin.com/in/rafael-franchini-37b0a21a4/",
                            );
                        },
                    );
                    ui.label(RichText::new("Desenvolvido por Rafael Franchini (R4LF)").size(11.5).color(MUTED));
                });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 600.0])
            .with_min_inner_size([440.0, 520.0])
            .with_title("PhoneMirror"),
        ..Default::default()
    };
    eframe::run_native("PhoneMirror", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
