use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

use phone_mirror_adb::{AdbClient, AdbError};
use phone_mirror_protocol::{DeviceMeta, ProtocolError};

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Erro no ADB: {0}")]
    Adb(#[from] AdbError),
    #[error("Erro no protocolo: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("Erro de I/O / socket: {0}")]
    Io(#[from] std::io::Error),
    #[error("Timeout aguardando conexão do servidor")]
    Timeout,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub serial: Option<String>,
    pub max_size: u16,
    pub bit_rate: u32,
    pub max_fps: u8,
    pub audio: bool,
    pub local_port: u16,
    pub server_asset_path: PathBuf,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            serial: None,
            max_size: 1920,
            bit_rate: 8_000_000,
            max_fps: 60,
            audio: false,
            local_port: 27183,
            server_asset_path: PathBuf::from(r"assets\scrcpy-server-v3.1"),
        }
    }
}

pub struct MirrorSession {
    config: SessionConfig,
    adb: AdbClient,
    pub meta: Option<DeviceMeta>,
}

impl MirrorSession {
    pub fn new(config: SessionConfig) -> Result<Self, SessionError> {
        let adb = AdbClient::new()?;
        Ok(Self {
            config,
            adb,
            meta: None,
        })
    }

    pub async fn start(&mut self) -> Result<(TcpStream, Option<TcpStream>, TcpStream), SessionError> {
        let remote_server_path = "/data/local/tmp/scrcpy-server.jar";
        let abstract_name = "scrcpy";

        info!("Enviando binário scrcpy-server para o dispositivo...");
        self.adb
            .push(
                self.config.serial.as_deref(),
                &self.config.server_asset_path,
                remote_server_path,
            )
            .await?;

        // 1. Inicia o listener TCP no PC
        let addr: SocketAddr = format!("127.0.0.1:{}", self.config.local_port).parse().unwrap();
        let listener = TcpListener::bind(addr).await?;
        info!("Listener TCP ativo no PC em {}", addr);

        // 2. Configura adb reverse explicitamente com o serial
        let adb_path = self.adb.with_path_str();
        let mut reverse_cmd = std::process::Command::new(&adb_path);
        if let Some(s) = self.config.serial.as_deref() {
            reverse_cmd.arg("-s").arg(s);
        }
        reverse_cmd
            .arg("reverse")
            .arg(format!("localabstract:{}", abstract_name))
            .arg(format!("tcp:{}", self.config.local_port));
        let rev_out = reverse_cmd.output()?;
        info!("Configuração de adb reverse: {}", String::from_utf8_lossy(&rev_out.stdout).trim());

        // 3. Dispara o servidor scrcpy no Android
        info!("Disparando processo do scrcpy-server no Android...");
        let mut spawn_cmd = std::process::Command::new(&adb_path);
        if let Some(s) = self.config.serial.as_deref() {
            spawn_cmd.arg("-s").arg(s);
        }

        let audio_flag = if self.config.audio { "audio=true" } else { "audio=false" };
        let scrcpy_args = format!(
            "video=true control=true {} max_size={} video_bit_rate={} max_fps={} send_device_meta=true send_frame_meta=true cleanup=false",
            audio_flag, self.config.max_size, self.config.bit_rate, self.config.max_fps
        );

        spawn_cmd
            .arg("shell")
            .arg(format!(
                "CLASSPATH={} app_process / com.genymobile.scrcpy.Server 3.1 {}",
                remote_server_path, scrcpy_args
            ))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Falha ao disparar processo no shell");

        info!("Aguardando conexão do socket de vídeo vinda do celular...");
        let (mut video_socket, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
            .await
            .map_err(|_| SessionError::Timeout)??;
        video_socket.set_nodelay(true)?;

        let mut dummy = [0u8; 1];
        video_socket.read_exact(&mut dummy).await?;

        let mut meta_buf = [0u8; 64 + 4 + 4 + 4];
        video_socket.read_exact(&mut meta_buf).await?;
        let mut cursor = std::io::Cursor::new(&meta_buf);
        let meta = DeviceMeta::parse_video_header(&mut cursor)?;
        info!("Metadados recebidos do celular com sucesso: {:?}", meta);
        self.meta = Some(meta);

        let audio_socket = if self.config.audio {
            info!("Aguardando conexão de áudio...");
            let (s, _) = listener.accept().await?;
            s.set_nodelay(true)?;
            Some(s)
        } else {
            None
        };

        info!("Aguardando conexão de controle...");
        let (control_socket, _) = listener.accept().await?;
        control_socket.set_nodelay(true)?;
        info!("Todos os canais conectados com sucesso!");

        Ok((video_socket, audio_socket, control_socket))
    }
}
