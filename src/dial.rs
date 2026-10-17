//! АлгедОнический метр: Киберфольк Бира, Чили, 1972.
//!
//! Бир планировал раздать чилийцам прибор со стрелкой: человек сам двигает её между
//! полным недовольством и полным довольством. **Прибор не задаёт вопросов.** Ни анкет,
//! ни оценок «по пятибалльной шкале», ни метрик, выведенных из системы.
//!
//! Здесь то же самое: файл `dial-<кто>` в среде, значение 0..100 и записка. Ставит
//! человек, читает система. Это единственный сигнал, который не выводится из данных —
//! он и есть слово человека.
//!
//! Два правила:
//! 1. **Показание стареет.** Настроение — не постоянный факт: если метр не подтвердили
//!    за полсуток, он возвращается к нулю. Так «когда-то было плохо» не живёт вечно.
//! 2. **Берётся худшее.** Если у троих 10, 20 и 70 — сигнал даёт 70. АлгедОника — это
//!    боль, а боль не усредняют.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::env::now;

/// Выше этого метр кричит: алгедОнический сигнал, не «замечание».
pub const PAIN: f64 = 60.0;

/// За полсуток показание без подтверждения гаснет.
pub const FADE_PER_SEC: f64 = 0.0001;

#[derive(Debug, Clone)]
pub struct Dial {
    pub by: String,
    pub value: f64,
    pub note: String,
    pub made: u64,
}

impl Dial {
    pub fn current(&self, at: u64) -> f64 {
        let age = at.saturating_sub(self.made) as f64;
        self.value * (-FADE_PER_SEC * age).exp()
    }

    pub fn is_pain(&self, at: u64) -> bool {
        self.current(at) >= PAIN
    }

    pub fn render(&self) -> String {
        format!(
            "by={}\nvalue={}\nnote={}\nmade={}\n",
            self.by, self.value, self.note, self.made
        )
    }

    pub fn parse(text: &str) -> Option<Dial> {
        let mut dial = Dial {
            by: String::new(),
            value: 0.0,
            note: String::new(),
            made: 0,
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "by" => dial.by = value.trim().to_string(),
                "value" => dial.value = value.trim().parse().unwrap_or(0.0),
                "note" => dial.note = value.trim().to_string(),
                "made" => dial.made = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        if dial.by.is_empty() {
            return None;
        }
        Some(dial)
    }
}

pub struct Meters {
    root: PathBuf,
}

impl Meters {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Meters> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("dials"))?;
        Ok(Meters { root })
    }

    fn dir(&self) -> PathBuf {
        self.root.join("dials")
    }

    /// Поставить стрелку. Человек делает это сам — прибор не спрашивает.
    pub fn set(&self, by: &str, value: f64, note: &str, at: u64) -> io::Result<Dial> {
        let dial = Dial {
            by: by.to_string(),
            value: value.clamp(0.0, 100.0),
            note: note.to_string(),
            made: at,
        };
        let name = by
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        fs::write(self.dir().join(format!("dial-{name}")), dial.render())?;
        Ok(dial)
    }

    pub fn all(&self) -> Vec<Dial> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.dir()) else {
            return out;
        };
        for entry in entries.flatten() {
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Some(dial) = Dial::parse(&text) {
                out.push(dial);
            }
        }
        out.sort_by(|a, b| a.by.cmp(&b.by));
        out
    }

    /// Худшее живое показание: алгедОнику не усредняют.
    pub fn worst(&self, at: u64) -> Option<Dial> {
        self.all()
            .into_iter()
            .filter(|dial| dial.current(at) > 0.5)
            .max_by(|a, b| {
                a.current(at)
                    .partial_cmp(&b.current(at))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    pub fn pain(&self, at: u64) -> Option<Dial> {
        self.worst(at).filter(|dial| dial.is_pain(at))
    }
}

pub fn now_or(at: Option<u64>) -> u64 {
    at.unwrap_or_else(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(name: &str) -> Meters {
        let dir = std::env::temp_dir().join(format!("stig-dial-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Meters::open(dir).unwrap()
    }

    #[test]
    fn the_dial_is_set_by_a_human_and_read_without_questions() {
        let m = meters("set");
        m.set("анна", 70.0, "не то делаем", 1000).unwrap();
        let worst = m.worst(1000).unwrap();
        assert_eq!(worst.by, "анна");
        assert_eq!(worst.note, "не то делаем");
        assert!(m.pain(1000).is_some(), "70 выше порога боли");
    }

    #[test]
    fn worst_reading_wins_and_is_not_averaged() {
        let m = meters("worst");
        m.set("анна", 10.0, "", 1000).unwrap();
        m.set("борис", 20.0, "", 1000).unwrap();
        m.set("вера", 75.0, "почти край", 1000).unwrap();
        let worst = m.worst(1000).unwrap();
        assert_eq!(worst.by, "вера", "боль не усредняют");
        assert!(worst.is_pain(1000));
    }

    #[test]
    fn reading_fades_and_must_be_reconfirmed() {
        let m = meters("fade");
        m.set("анна", 80.0, "плохо", 1000).unwrap();
        assert!(m.pain(1000 + 600).is_some(), "через десять минут ещё болит");
        assert!(
            m.pain(1000 + 3600).is_none(),
            "через час 80→55: порог уже не пройден, но показание видно"
        );
        assert!(
            m.pain(1000 + 86_400).is_none(),
            "через сутки без подтверждения — уже не факт"
        );
    }

    #[test]
    fn value_is_clamped_and_roundtrips() {
        let m = meters("clamp");
        let dial = m.set("анна", 180.0, "", 1000).unwrap();
        assert_eq!(dial.value, 100.0);
        let back = Dial::parse(&dial.render()).unwrap();
        assert_eq!(back.by, "анна");
        assert!((back.value - 100.0).abs() < 1e-9);
    }
}
