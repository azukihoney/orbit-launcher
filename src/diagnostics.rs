use crate::model::Settings;
use serde::Serialize;
#[derive(Serialize)]
pub struct Sample {
    t: f64,
    point: Option<[i32; 2]>,
    state: String,
    recognized: bool,
}
pub struct Capture {
    pub started: f64,
    origin: Option<(i32, i32)>,
    pub samples: Vec<Sample>,
    settings: Settings,
}
impl Capture {
    pub fn new(now: f64, settings: Settings) -> Self {
        Self {
            started: now,
            origin: None,
            samples: vec![],
            settings,
        }
    }
    pub fn record(&mut self, now: f64, point: Option<(i32, i32)>, state: &str, recognized: bool) {
        if now - self.started > 20.1 || self.samples.len() >= 4096 {
            return;
        }
        if self.origin.is_none() {
            self.origin = point;
        }
        let relative = point.zip(self.origin).map(|(p, o)| [p.0 - o.0, p.1 - o.1]);
        self.samples.push(Sample {
            t: now - self.started,
            point: relative,
            state: state.into(),
            recognized,
        });
    }
    pub fn save(&self, folder: &std::path::Path) -> Result<std::path::PathBuf, String> {
        use std::io::Write;
        std::fs::create_dir_all(folder).map_err(|_| "Capture directory creation failed")?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = folder.join(format!("gesture-{timestamp}.json"));
        let recognized = self.samples.iter().filter(|s| s.recognized).count();
        let value = serde_json::json!({"version":1,"coordinate_system":"relative_screen_pixels",
            "settings":self.settings,"summary":{"samples":self.samples.len(),"recognized":recognized},"samples":self.samples});
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "Capture file creation failed")?;
        f.write_all(
            &serde_json::to_vec_pretty(&value).map_err(|_| "Capture serialization failed")?,
        )
        .map_err(|_| "Capture writing failed")?;
        Ok(path)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_relative_coordinates_are_serialized() {
        let mut c = Capture::new(100., Settings::default());
        c.record(100., Some((3247, 1999)), "tracking", false);
        c.record(100.1, Some((3257, 1994)), "tracking", true);
        let json = serde_json::to_value(&c.samples).unwrap();
        assert_eq!(json[0]["point"], serde_json::json!([0, 0]));
        assert_eq!(json[1]["point"], serde_json::json!([10, -5]));
        assert!(!json.to_string().contains("3247"));
        c.record(121., Some((1, 1)), "tracking", false);
        assert_eq!(c.samples.len(), 2);
    }
}
