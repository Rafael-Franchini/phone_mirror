use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::process::Command;

pub const DISCOVERY_PORT: u16 = 45454;
pub const PC_NOTIFY_PORT: u16 = 45455;

#[derive(Error, Debug)]
pub enum AdbError {
    #[error("ADB binário não encontrado")]
    NotFound,
    #[error("Erro de execução do ADB: {0}")]
    ProcessFailed(String),
    #[error("Erro de I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Dispositivo não autorizado ou offline")]
    UnauthorizedOrOffline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    pub model: Option<String>,
    pub is_wireless: bool,
}

#[derive(Clone, Debug)]
pub struct AdbClient {
    adb_path: PathBuf,
}

impl AdbClient {
    pub fn new() -> Result<Self, AdbError> {
        // 1. Variável de ambiente ADB_PATH
        if let Ok(env_path) = std::env::var("ADB_PATH") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Ok(Self { adb_path: p });
            }
        }

        // 2. WinGet scrcpy package (muito comum no Windows)
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let winget_scrcpy = PathBuf::from(&local_app_data)
                .join(r"Microsoft\WinGet\Packages\Genymobile.scrcpy_Microsoft.Winget.Source_8wekyb3d8bbwe\scrcpy-win64-v5.0.1\adb.exe");
            if winget_scrcpy.exists() {
                return Ok(Self { adb_path: winget_scrcpy });
            }

            let sdk_adb = PathBuf::from(&local_app_data).join(r"Android\Sdk\platform-tools\adb.exe");
            if sdk_adb.exists() {
                return Ok(Self { adb_path: sdk_adb });
            }
        }

        // 3. ANDROID_HOME
        if let Ok(android_home) = std::env::var("ANDROID_HOME") {
            let home_adb = PathBuf::from(android_home).join(r"platform-tools\adb.exe");
            if home_adb.exists() {
                return Ok(Self { adb_path: home_adb });
            }
        }

        // 4. Default no PATH
        Ok(Self { adb_path: PathBuf::from("adb") })
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(&self.adb_path);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }

    pub fn with_path_str(&self) -> String {
        self.adb_path.to_string_lossy().to_string()
    }

    pub fn get_adb_path(&self) -> &Path {
        &self.adb_path
    }

    pub async fn push(&self, serial: Option<&str>, src: &Path, dst: &str) -> Result<(), AdbError> {
        let mut cmd = self.cmd();
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.arg("push").arg(src).arg(dst);

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(AdbError::ProcessFailed(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        Ok(())
    }

    pub async fn list_devices(&self) -> Result<Vec<AdbDevice>, AdbError> {
        let mut cmd = self.cmd();
        cmd.arg("devices").arg("-l");

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(AdbError::ProcessFailed(String::from_utf8_lossy(&output.stderr).to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("List of devices") || line.starts_with("* daemon") {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let serial = parts[0].to_string();
                let state = parts[1].to_string();
                let is_wireless = serial.contains(':');

                let mut model = None;
                for part in &parts[2..] {
                    if let Some(m) = part.strip_prefix("model:") {
                        model = Some(m.to_string());
                        break;
                    }
                }

                devices.push(AdbDevice {
                    serial,
                    state,
                    model,
                    is_wireless,
                });
            }
        }

        // Busca o nome amigável do aparelho (ex: "Galaxy S23") nos aparelhos já conectados
        for dev in &mut devices {
            if dev.state == "device" {
                if let Ok(Some(friendly)) = self.get_friendly_name(&dev.serial).await {
                    dev.model = Some(friendly);
                }
            }
        }

        Ok(devices)
    }

    pub async fn get_friendly_name(&self, serial: &str) -> Result<Option<String>, AdbError> {
        let mut cmd = self.cmd();
        cmd.arg("-s")
            .arg(serial)
            .arg("shell")
            .arg("settings get global device_name");
        if let Ok(out) = cmd.output().await {
            if out.status.success() {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !name.is_empty() && name != "null" {
                    return Ok(Some(name));
                }
            }
        }
        Ok(None)
    }

    /// Faz varredura na rede Wi-Fi procurando aparelhos rodando o app PhoneMirror.
    pub async fn scan_wifi_companions(&self) -> Vec<AdbDevice> {
        tokio::task::spawn_blocking(scan_udp_companions_blocking)
            .await
            .unwrap_or_default()
    }

    pub async fn enable_tcpip(&self, serial: Option<&str>, port: u16) -> Result<String, AdbError> {
        let mut cmd = self.cmd();
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.arg("tcpip").arg(port.to_string());

        let output = cmd.output().await?;
        let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AdbError::ProcessFailed(if err.is_empty() { msg } else { err }));
        }
        Ok(msg)
    }

    pub async fn connect_tcp(&self, ip: &str, port: u16) -> Result<String, AdbError> {
        let mut cmd = self.cmd();
        let target = format!("{}:{}", ip.trim(), port);
        cmd.arg("connect").arg(&target);

        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() || stdout.contains("failed") || stdout.contains("unable to connect") {
            return Err(AdbError::ProcessFailed(if stderr.is_empty() { stdout } else { stderr }));
        }

        Ok(stdout)
    }

    pub async fn disconnect(&self, target: &str) -> Result<String, AdbError> {
        let mut cmd = self.cmd();
        cmd.arg("disconnect").arg(target.trim());

        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    }

    pub async fn get_device_ip(&self, serial: Option<&str>) -> Result<Option<String>, AdbError> {
        let mut cmd = self.cmd();
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.arg("shell").arg("ip").arg("route");

        let output = cmd.output().await?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Ex: "192.168.1.0/24 dev wlan0 proto kernel scope link src 192.168.1.110"
                if line.contains("wlan0") && line.contains("src ") {
                    if let Some(idx) = line.find("src ") {
                        let rest = &line[idx + 4..].trim();
                        let ip = rest.split_whitespace().next().unwrap_or("");
                        if !ip.is_empty() {
                            return Ok(Some(ip.to_string()));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

pub fn parse_companion_packet(msg: &str, fallback_ip: Ipv4Addr) -> Option<AdbDevice> {
    let line = msg.trim();
    if !line.starts_with("PHONEMIRROR_HERE|") {
        return None;
    }
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 4 {
        return None;
    }
    let name = parts[1].trim();
    let ip_str = parts[2].trim();
    let port: u16 = parts[3].trim().parse().unwrap_or(5555);
    let ip = if ip_str.is_empty() || ip_str == "0.0.0.0" {
        fallback_ip.to_string()
    } else {
        ip_str.to_string()
    };

    Some(AdbDevice {
        serial: format!("{}:{}", ip, port),
        state: "companion".to_string(),
        model: Some(if name.is_empty() {
            "Android (PhoneMirror)".to_string()
        } else {
            name.to_string()
        }),
        is_wireless: true,
    })
}

fn local_ipv4_prefixes() -> Vec<[u8; 3]> {
    let mut prefixes = Vec::new();

    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(SocketAddr::V4(local)) = sock.local_addr() {
                let oct = local.ip().octets();
                if oct[0] != 127 {
                    prefixes.push([oct[0], oct[1], oct[2]]);
                }
            }
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("ipconfig");
        cmd.creation_flags(0x0800_0000);
        if let Ok(out) = cmd.output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.contains("IPv4") {
                    if let Some(colon) = line.rfind(':') {
                        let ip_part = line[colon + 1..].trim();
                        if let Ok(ip) = ip_part.parse::<Ipv4Addr>() {
                            let oct = ip.octets();
                            if oct[0] != 127 && oct[0] != 169 {
                                let p = [oct[0], oct[1], oct[2]];
                                if !prefixes.contains(&p) {
                                    prefixes.push(p);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    prefixes
}

pub fn scan_udp_companions_blocking() -> Vec<AdbDevice> {
    let mut found: HashMap<String, AdbDevice> = HashMap::new();
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(80)));

    let probe = b"PHONEMIRROR_DISCOVER";
    let _ = sock.send_to(probe, SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_PORT));

    let prefixes = local_ipv4_prefixes();
    for p in &prefixes {
        let bcast = Ipv4Addr::new(p[0], p[1], p[2], 255);
        let _ = sock.send_to(probe, SocketAddrV4::new(bcast, DISCOVERY_PORT));
        for host in 1..=254u8 {
            let target = Ipv4Addr::new(p[0], p[1], p[2], host);
            let _ = sock.send_to(probe, SocketAddrV4::new(target, DISCOVERY_PORT));
        }
    }

    let deadline = Instant::now() + Duration::from_millis(400);
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((len, SocketAddr::V4(src))) => {
                if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                    if let Some(dev) = parse_companion_packet(text, *src.ip()) {
                        found.insert(dev.serial.clone(), dev);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {
                if Instant::now() >= deadline {
                    break;
                }
            }
        }
    }

    found.into_values().collect()
}

/// Pede permissão ao aplicativo no celular para iniciar a transmissão de tela.
/// Retorna `Ok(true)` se o usuário aceitou no celular, `Ok(false)` se recusou, ou `Err` em timeout.
pub fn request_stream_authorization_blocking(ip: &str) -> Result<bool, String> {
    let Ok(target_ip) = ip.trim().parse::<Ipv4Addr>() else {
        return Err("Endereço IP do celular inválido.".into());
    };
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return Err("Falha ao abrir canal de autorização UDP.".into());
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));

    let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    let req = format!("PHONEMIRROR_REQUEST_STREAM|{}", pc_name);
    let dest = SocketAddrV4::new(target_ip, DISCOVERY_PORT);

    let deadline = Instant::now() + Duration::from_secs(25);
    let mut last_send = Instant::now() - Duration::from_secs(5);
    let mut buf = [0u8; 256];

    while Instant::now() < deadline {
        if last_send.elapsed() >= Duration::from_millis(450) {
            let _ = sock.send_to(req.as_bytes(), dest);
            last_send = Instant::now();
        }
        if let Ok((len, _)) = sock.recv_from(&mut buf) {
            if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                let msg = text.trim();
                if msg.starts_with("PHONEMIRROR_ACCEPT") {
                    return Ok(true);
                } else if msg.starts_with("PHONEMIRROR_REJECT") {
                    return Ok(false);
                }
            }
        }
    }

    Err("Tempo esgotado aguardando você aceitar a transmissão na tela do celular.".into())
}

pub fn notify_pc_stopped(ip: &str) {
    if let Ok(target_ip) = ip.trim().parse::<Ipv4Addr>() {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            let dest = SocketAddrV4::new(target_ip, DISCOVERY_PORT);
            let _ = sock.send_to(b"PHONEMIRROR_PC_STOP", dest);
        }
    }
}

pub fn check_phone_stopped_blocking(ip: &str) -> bool {
    let Ok(target_ip) = ip.trim().parse::<Ipv4Addr>() else {
        return false;
    };
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return false;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let dest = SocketAddrV4::new(target_ip, DISCOVERY_PORT);
    let _ = sock.send_to(b"PHONEMIRROR_PING", dest);
    let mut buf = [0u8; 128];
    if let Ok((len, _)) = sock.recv_from(&mut buf) {
        if let Ok(text) = std::str::from_utf8(&buf[..len]) {
            if text.trim().starts_with("PHONEMIRROR_STOP") {
                return true;
            }
        }
    }
    false
}
