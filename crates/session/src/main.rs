use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use phone_mirror_input::{Binding, KeymapEngine, KeymapProfile};
use phone_mirror_protocol::PacketHeader;
use phone_mirror_session::{MirrorSession, SessionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("=== Iniciando PhoneMirror CLI Test ===");

    let mut config = SessionConfig::default();
    config.server_asset_path = PathBuf::from(r"assets\scrcpy-server-v3.1");

    let mut session = MirrorSession::new(config)?;
    let (mut video_socket, _audio_socket, mut control_socket) = session.start().await?;

    info!("Sessão conectada! Dispositivo: {:?}", session.meta);

    let meta = session.meta.as_ref().unwrap();
    let screen_w = meta.width as u16;
    let screen_h = meta.height as u16;

    // Inicializar motor de keymapping
    let mut keymap = KeymapEngine::new();
    let profile = KeymapProfile {
        package_name: "test.game".to_string(),
        profile_name: "WASD Profile".to_string(),
        mouse_lock_toggle_key: "Tab".to_string(),
        bindings: vec![
            Binding::Tap {
                key: "Space".to_string(),
                pos: [0.5, 0.5], // Toque no centro
            },
            Binding::Joystick {
                up: "W".to_string(),
                left: "A".to_string(),
                down: "S".to_string(),
                right: "D".to_string(),
                center: [0.2, 0.7],
                radius: 0.08,
            },
        ],
    };
    keymap.set_profile(profile);

    // Spawn thread de leitura de frames de vídeo
    let video_handle = tokio::spawn(async move {
        let mut total_packets = 0;
        let mut total_bytes = 0;
        let start = std::time::Instant::now();

        loop {
            let mut header_buf = [0u8; 12];
            if let Err(e) = video_socket.read_exact(&mut header_buf).await {
                info!("Fim do stream de vídeo ou erro: {:?}", e);
                break;
            }

            let mut cursor = std::io::Cursor::new(&header_buf);
            let header = match PacketHeader::parse(&mut cursor) {
                Ok(h) => h,
                Err(e) => {
                    info!("Erro ao ler header do pacote: {:?}", e);
                    break;
                }
            };

            let mut payload = vec![0u8; header.packet_size as usize];
            if let Err(e) = video_socket.read_exact(&mut payload).await {
                info!("Erro ao ler payload: {:?}", e);
                break;
            }

            total_packets += 1;
            total_bytes += header.packet_size;

            if total_packets % 30 == 0 {
                info!(
                    "Recebidos {} pacotes de vídeo (~{:.2} KB). Último pacote: pts={} key_frame={}",
                    total_packets,
                    total_bytes as f64 / 1024.0,
                    header.pts,
                    header.is_key_frame
                );
            }

            if start.elapsed() > Duration::from_secs(5) {
                info!("Captura de 5 segundos de vídeo concluída com sucesso!");
                break;
            }
        }
    });

    // Testar envio de controles mapeados
    info!("Simulando toques e movimentos de WASD no aparelho...");
    let key_events = vec![
        ("Space", true),
        ("Space", false),
        ("W", true),
        ("D", true),
        ("W", false),
        ("D", false),
    ];

    for (key, is_down) in key_events {
        let msgs = keymap.process_key_event(key, is_down, screen_w, screen_h);
        for msg in msgs {
            let mut buf = Vec::new();
            msg.serialize(&mut buf)?;
            control_socket.write_all(&buf).await?;
            info!("Enviado evento de controle: {:?} (key={}, down={})", msg, key, is_down);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    let _ = video_handle.await;
    info!("=== Teste concluído com êxito! ===");

    Ok(())
}
