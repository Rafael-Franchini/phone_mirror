use std::collections::HashMap;
use phone_mirror_protocol::{ControlMsg, MotionAction, Position};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Binding {
    Tap {
        key: String,
        pos: [f32; 2], // [x_norm, y_norm]
    },
    Joystick {
        up: String,
        left: String,
        down: String,
        right: String,
        center: [f32; 2],
        radius: f32,
    },
    Camera {
        area_center: [f32; 2],
        sensitivity: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeymapProfile {
    pub package_name: String,
    pub profile_name: String,
    pub mouse_lock_toggle_key: String,
    pub bindings: Vec<Binding>,
}

pub struct KeymapEngine {
    profile: Option<KeymapProfile>,
    mouse_locked: bool,
    active_pointers: HashMap<String, u64>,
    next_pointer_id: u64,
}

impl KeymapEngine {
    pub fn new() -> Self {
        Self {
            profile: None,
            mouse_locked: false,
            active_pointers: HashMap::new(),
            next_pointer_id: 10,
        }
    }

    pub fn set_profile(&mut self, profile: KeymapProfile) {
        self.profile = Some(profile);
        self.active_pointers.clear();
    }

    pub fn is_mouse_locked(&self) -> bool {
        self.mouse_locked
    }

    pub fn toggle_mouse_lock(&mut self) -> bool {
        self.mouse_locked = !self.mouse_locked;
        self.mouse_locked
    }

    pub fn process_key_event(
        &mut self,
        key: &str,
        is_down: bool,
        screen_w: u16,
        screen_h: u16,
    ) -> Vec<ControlMsg> {
        let mut msgs = Vec::new();
        let profile = match &self.profile {
            Some(p) => p,
            None => return msgs,
        };

        if is_down && key.eq_ignore_ascii_case(&profile.mouse_lock_toggle_key) {
            self.toggle_mouse_lock();
            return msgs;
        }

        for binding in &profile.bindings {
            match binding {
                Binding::Tap { key: b_key, pos } => {
                    if b_key.eq_ignore_ascii_case(key) {
                        let pointer_id = if is_down {
                            let id = self.next_pointer_id;
                            self.next_pointer_id += 1;
                            self.active_pointers.insert(key.to_string(), id);
                            id
                        } else {
                            self.active_pointers.remove(key).unwrap_or(1)
                        };

                        let action = if is_down {
                            MotionAction::Down
                        } else {
                            MotionAction::Up
                        };

                        msgs.push(ControlMsg::InjectTouch {
                            action,
                            pointer_id,
                            position: Position::normalized(pos[0], pos[1], screen_w, screen_h),
                            pressure: if is_down { 1.0 } else { 0.0 },
                            action_button: 1,
                            buttons: if is_down { 1 } else { 0 },
                        });
                    }
                }
                Binding::Joystick { up, left, down, right, center, radius } => {
                    let is_direction = key.eq_ignore_ascii_case(up)
                        || key.eq_ignore_ascii_case(left)
                        || key.eq_ignore_ascii_case(down)
                        || key.eq_ignore_ascii_case(right);

                    if is_direction {
                        let joystick_key = "virtual_joystick";
                        let pointer_id = *self.active_pointers.entry(joystick_key.to_string()).or_insert_with(|| {
                            let id = self.next_pointer_id;
                            self.next_pointer_id += 1;
                            id
                        });

                        let mut dx = 0.0f32;
                        let mut dy = 0.0f32;

                        if key.eq_ignore_ascii_case(up) && is_down { dy -= radius; }
                        if key.eq_ignore_ascii_case(down) && is_down { dy += radius; }
                        if key.eq_ignore_ascii_case(left) && is_down { dx -= radius; }
                        if key.eq_ignore_ascii_case(right) && is_down { dx += radius; }

                        let norm_x = center[0] + dx;
                        let norm_y = center[1] + dy;

                        let action = if is_down {
                            MotionAction::Move
                        } else {
                            MotionAction::Up
                        };

                        msgs.push(ControlMsg::InjectTouch {
                            action,
                            pointer_id,
                            position: Position::normalized(norm_x, norm_y, screen_w, screen_h),
                            pressure: if is_down { 1.0 } else { 0.0 },
                            action_button: 1,
                            buttons: if is_down { 1 } else { 0 },
                        });
                    }
                }
                _ => {}
            }
        }

        msgs
    }

    pub fn process_mouse_move(
        &mut self,
        rel_x: f32,
        rel_y: f32,
        screen_w: u16,
        screen_h: u16,
    ) -> Option<ControlMsg> {
        if !self.mouse_locked {
            return None;
        }

        let profile = self.profile.as_ref()?;
        for binding in &profile.bindings {
            if let Binding::Camera { area_center, sensitivity } = binding {
                let pointer_id = 999;
                let target_x = area_center[0] + (rel_x * sensitivity / screen_w as f32);
                let target_y = area_center[1] + (rel_y * sensitivity / screen_h as f32);

                return Some(ControlMsg::InjectTouch {
                    action: MotionAction::Move,
                    pointer_id,
                    position: Position::normalized(target_x, target_y, screen_w, screen_h),
                    pressure: 1.0,
                    action_button: 1,
                    buttons: 1,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tap_mapping() {
        let mut engine = KeymapEngine::new();
        let profile = KeymapProfile {
            package_name: "com.pubg.imobile".to_string(),
            profile_name: "Default PUBG".to_string(),
            mouse_lock_toggle_key: "Tab".to_string(),
            bindings: vec![Binding::Tap {
                key: "Space".to_string(),
                pos: [0.85, 0.75],
            }],
        };
        engine.set_profile(profile);

        let msgs = engine.process_key_event("Space", true, 1920, 1080);
        assert_eq!(msgs.len(), 1);
        if let ControlMsg::InjectTouch { action, position, .. } = &msgs[0] {
            assert_eq!(*action, MotionAction::Down);
            assert_eq!(position.x, (0.85 * 1920.0) as i32);
            assert_eq!(position.y, (0.75 * 1080.0) as i32);
        } else {
            panic!("Esperava InjectTouch");
        }
    }

    #[test]
    fn test_mouse_lock_toggle() {
        let mut engine = KeymapEngine::new();
        let profile = KeymapProfile {
            package_name: "com.riotgames".to_string(),
            profile_name: "FPS".to_string(),
            mouse_lock_toggle_key: "Tab".to_string(),
            bindings: vec![],
        };
        engine.set_profile(profile);

        assert!(!engine.is_mouse_locked());
        engine.process_key_event("Tab", true, 1920, 1080);
        assert!(engine.is_mouse_locked());
        engine.process_key_event("Tab", true, 1920, 1080);
        assert!(!engine.is_mouse_locked());
    }
}
