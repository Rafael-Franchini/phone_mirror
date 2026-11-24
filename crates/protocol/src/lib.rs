use std::io::{self, Read, Write};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use thiserror::Error;

pub const SCRCPY_VERSION: &str = "3.1";
pub const DEVICE_NAME_FIELD_LENGTH: usize = 64;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Erro de I/O de rede ou buffer: {0}")]
    Io(#[from] io::Error),
    #[error("Comprimento de string UTF-8 inválido ou corrompido")]
    InvalidUtf8,
    #[error("Tipo de mensagem de controle desconhecido: {0}")]
    UnknownControlMsgType(u8),
    #[error("Tipo de ação inválida: {0}")]
    InvalidAction(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Down = 0,
    Up = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionAction {
    Down = 0,
    Up = 1,
    Move = 2,
    Cancel = 3,
    Outside = 4,
    PointerDown = 5,
    PointerUp = 6,
    HoverMove = 7,
    Scroll = 8,
    HoverEnter = 9,
    HoverExit = 10,
    ButtonPress = 11,
    ButtonRelease = 12,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub screen_width: u16,
    pub screen_height: u16,
}

impl Position {
    pub fn normalized(norm_x: f32, norm_y: f32, screen_width: u16, screen_height: u16) -> Self {
        let x = (norm_x.clamp(0.0, 1.0) * screen_width as f32) as i32;
        let y = (norm_y.clamp(0.0, 1.0) * screen_height as f32) as i32;
        Self { x, y, screen_width, screen_height }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlMsg {
    InjectKeycode {
        action: KeyAction,
        keycode: u32,
        repeat: u32,
        meta_state: u32,
    },
    InjectTouch {
        action: MotionAction,
        pointer_id: u64,
        position: Position,
        pressure: f32,
        action_button: u32,
        buttons: u32,
    },
    InjectScroll {
        position: Position,
        hscroll: i32,
        vscroll: i32,
        buttons: u32,
    },
    BackOrScreenOn {
        action: KeyAction,
    },
    SetScreenPowerMode {
        mode: u8, // 0 = off, 2 = normal
    },
    UhidCreate {
        id: u16,
        report_desc: Vec<u8>,
    },
    UhidInput {
        id: u16,
        data: Vec<u8>,
    },
    UhidDestroy {
        id: u16,
    },
}

impl ControlMsg {
    pub fn serialize(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            ControlMsg::InjectKeycode { action, keycode, repeat, meta_state } => {
                buf.write_u8(0)?; // TYPE_INJECT_KEYCODE
                buf.write_u8(*action as u8)?;
                buf.write_u32::<BigEndian>(*keycode)?;
                buf.write_u32::<BigEndian>(*repeat)?;
                buf.write_u32::<BigEndian>(*meta_state)?;
            }
            ControlMsg::InjectTouch { action, pointer_id, position, pressure, action_button, buttons } => {
                buf.write_u8(2)?; // TYPE_INJECT_TOUCH_EVENT
                buf.write_u8(*action as u8)?;
                buf.write_u64::<BigEndian>(*pointer_id)?;
                buf.write_i32::<BigEndian>(position.x)?;
                buf.write_i32::<BigEndian>(position.y)?;
                buf.write_u16::<BigEndian>(position.screen_width)?;
                buf.write_u16::<BigEndian>(position.screen_height)?;
                let pressure_u16 = (pressure.clamp(0.0, 1.0) * 65535.0) as u16;
                buf.write_u16::<BigEndian>(pressure_u16)?;
                buf.write_u32::<BigEndian>(*action_button)?;
                buf.write_u32::<BigEndian>(*buttons)?;
            }
            ControlMsg::InjectScroll { position, hscroll, vscroll, buttons } => {
                buf.write_u8(3)?; // TYPE_INJECT_SCROLL_EVENT
                buf.write_i32::<BigEndian>(position.x)?;
                buf.write_i32::<BigEndian>(position.y)?;
                buf.write_u16::<BigEndian>(position.screen_width)?;
                buf.write_u16::<BigEndian>(position.screen_height)?;
                buf.write_i32::<BigEndian>(*hscroll)?;
                buf.write_i32::<BigEndian>(*vscroll)?;
                buf.write_u32::<BigEndian>(*buttons)?;
            }
            ControlMsg::BackOrScreenOn { action } => {
                buf.write_u8(4)?; // TYPE_BACK_OR_SCREEN_ON
                buf.write_u8(*action as u8)?;
            }
            ControlMsg::SetScreenPowerMode { mode } => {
                buf.write_u8(7)?; // TYPE_SET_SCREEN_POWER_MODE
                buf.write_u8(*mode)?;
            }
            ControlMsg::UhidCreate { id, report_desc } => {
                buf.write_u8(13)?; // TYPE_UHID_CREATE
                buf.write_u16::<BigEndian>(*id)?;
                buf.write_u16::<BigEndian>(report_desc.len() as u16)?;
                buf.write_all(report_desc)?;
            }
            ControlMsg::UhidInput { id, data } => {
                buf.write_u8(14)?; // TYPE_UHID_INPUT
                buf.write_u16::<BigEndian>(*id)?;
                buf.write_u16::<BigEndian>(data.len() as u16)?;
                buf.write_all(data)?;
            }
            ControlMsg::UhidDestroy { id } => {
                buf.write_u8(15)?; // TYPE_UHID_DESTROY
                buf.write_u16::<BigEndian>(*id)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMeta {
    pub device_name: String,
    pub codec_id: u32,
    pub width: u32,
    pub height: u32,
}

impl DeviceMeta {
    pub fn parse_video_header<R: Read>(reader: &mut R) -> Result<Self, ProtocolError> {
        let mut name_buf = [0u8; DEVICE_NAME_FIELD_LENGTH];
        reader.read_exact(&mut name_buf)?;
        let end_idx = name_buf.iter().position(|&b| b == 0).unwrap_or(DEVICE_NAME_FIELD_LENGTH);
        let device_name = String::from_utf8_lossy(&name_buf[..end_idx]).to_string();

        let codec_id = reader.read_u32::<BigEndian>()?;
        let width = reader.read_u32::<BigEndian>()?;
        let height = reader.read_u32::<BigEndian>()?;

        Ok(Self {
            device_name,
            codec_id,
            width,
            height,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub pts: u64,
    pub is_config: bool,
    pub is_key_frame: bool,
    pub packet_size: u32,
}

impl PacketHeader {
    pub fn parse<R: Read>(reader: &mut R) -> Result<Self, ProtocolError> {
        let pts_flags = reader.read_u64::<BigEndian>()?;
        let packet_size = reader.read_u32::<BigEndian>()?;

        let is_config = (pts_flags & (1 << 63)) != 0;
        let is_key_frame = (pts_flags & (1 << 62)) != 0;
        let pts = pts_flags & !(0x3 << 62);

        Ok(Self {
            pts,
            is_config,
            is_key_frame,
            packet_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_inject_touch() {
        let pos = Position { x: 500, y: 1000, screen_width: 1080, screen_height: 2400 };
        let msg = ControlMsg::InjectTouch {
            action: MotionAction::Down,
            pointer_id: 1,
            position: pos,
            pressure: 1.0,
            action_button: 1,
            buttons: 1,
        };
        let mut buf = Vec::new();
        msg.serialize(&mut buf).unwrap();

        assert_eq!(buf[0], 2); // TYPE_INJECT_TOUCH_EVENT
        assert_eq!(buf[1], 0); // MotionAction::Down
        assert_eq!(buf.len(), 32);
    }

    #[test]
    fn test_packet_header_parse() {
        let mut bytes = Vec::new();
        let pts: u64 = 1234567;
        let flag_key: u64 = 1 << 62;
        bytes.write_u64::<BigEndian>(pts | flag_key).unwrap();
        bytes.write_u32::<BigEndian>(4096).unwrap();

        let mut cursor = std::io::Cursor::new(bytes);
        let header = PacketHeader::parse(&mut cursor).unwrap();

        assert_eq!(header.pts, 1234567);
        assert!(header.is_key_frame);
        assert!(!header.is_config);
        assert_eq!(header.packet_size, 4096);
    }
}
