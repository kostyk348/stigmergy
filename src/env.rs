//! Стигмергия: координация следами в среде, без доски и без очереди.
//!
//! Термин Пьер-Поль Грассе ввёл в 1959 году, изучая термитов: частичная постройка
//! сама вызывает следующее действие, прямой связи между термитами нет. Работа —
//! след в среде — и есть сигнал.
//!
//! Здесь нет ни карточек, ни колонок, ни списка задач, ни владельца. Среда — каталог
//! `traces/`, а работа существует как **потребность** (`need`) с силой. Сила убывает
//! со временем по `strength * exp(-decay * age)`: брошенная потребность гаснет сама,
//! без чьей-то воли. Исполнитель — любой процесс, у которого есть способность
//! (`capability`); он не получает задание, а **притягивается** к самому сильному
//! следу, который совпадает с его способностью.
//!
//! Захват работы — тоже след, только короткоживущий (`claim`, распад за минуту).
//! Умер процесс — след испарился, потребность снова притягивает. Аренды без владельца
//! и без уборщика.
//!
//! Формат следа — плоский текст `ключ=значение`, без зависимостей и без JSON:
//!
//! ```text
//! kind=need
//! need=rust
//! object=разобрать RFC 791
//! strength=1.0
//! decay=0.00008
//! made=1700000000
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// След слабее этого считается испарённым и удаляется обходом.
pub const EVAPORATED: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Потребность: работа, которая притягивает исполнителя.
    Need,
    /// Короткая метка «взято»: гасит притяжение для остальных.
    Claim,
    /// Сделано: остаётся как след работы для аудита.
    Done,
    /// Предупреждение: работа пыталась пройти и не смогла — отталкивает других.
    Warn,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Need => "need",
            Kind::Claim => "claim",
            Kind::Done => "done",
            Kind::Warn => "warn",
        }
    }
    fn parse(text: &str) -> Option<Kind> {
        match text {
            "need" => Some(Kind::Need),
            "claim" => Some(Kind::Claim),
            "done" => Some(Kind::Done),
            "warn" => Some(Kind::Warn),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trace {
    pub id: String,
    pub kind: Kind,
    /// Способность, которой нужна работа (`need`).
    pub need: String,
    pub object: String,
    pub strength: f64,
    /// Доля силы, теряемая за секунду.
    pub decay: f64,
    pub made: u64,
    pub by: String,
}

impl Trace {
    pub fn new(kind: Kind, need: &str, object: &str, by: &str, now: u64) -> Trace {
        let (strength, decay) = match kind {
            Kind::Need => (1.0, 0.00008), // ~полдня до испарения
            Kind::Claim => (1.0, 0.04),   // минута
            Kind::Done => (0.6, 0.00002), // долгая память работы
            Kind::Warn => (0.8, 0.003),   // ~двадцать пять минут отталкивания
        };
        Trace {
            id: format!("{}-{}", now, sanitize(object)),
            kind,
            need: need.to_string(),
            object: object.to_string(),
            strength,
            decay,
            made: now,
            by: by.to_string(),
        }
    }

    /// Сила следа сейчас. Это и есть приоритет: никакого другого нет.
    pub fn urgency(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.made) as f64;
        self.strength * (-self.decay * age).exp()
    }

    pub fn evaporated(&self, now: u64) -> bool {
        self.urgency(now) < EVAPORATED
    }

    pub fn render(&self) -> String {
        format!(
            "kind={}\nneed={}\nobject={}\nstrength={}\ndecay={}\nmade={}\nby={}\n",
            self.kind.as_str(),
            self.need,
            self.object,
            self.strength,
            self.decay,
            self.made,
            self.by
        )
    }

    pub fn parse(text: &str) -> Option<Trace> {
        let mut kind = None;
        let mut need = String::new();
        let mut object = String::new();
        let mut strength = 1.0;
        let mut decay = 0.00008;
        let mut made = 0;
        let mut by = String::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "kind" => kind = Kind::parse(value),
                "need" => need = value.to_string(),
                "object" => object = value.to_string(),
                "strength" => strength = value.parse().unwrap_or(1.0),
                "decay" => decay = value.parse().unwrap_or(0.00008),
                "made" => made = value.parse().unwrap_or(0),
                "by" => by = value.to_string(),
                _ => {}
            }
        }
        Some(Trace {
            id: String::new(),
            kind: kind?,
            need,
            object,
            strength,
            decay,
            made,
            by,
        })
    }
}

fn sanitize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    cleaned.trim_matches('-').chars().take(40).collect()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Среда: каталог со следами. Больше ничего.
pub struct Env {
    root: PathBuf,
}

impl Env {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Env> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("traces"))?;
        Ok(Env { root })
    }

    pub fn traces_dir(&self) -> PathBuf {
        self.root.join("traces")
    }

    /// Все следы среды, включая испарённые (их удаляет [`Env::sweep`]).
    pub fn traces(&self) -> Vec<Trace> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.traces_dir()) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("trace") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(mut trace) = Trace::parse(&text) {
                trace.id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                out.push(trace);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Записать новый след. Имя файла — уникальное, чтобы двое не затирали друг друга.
    pub fn leave(&self, trace: &Trace) -> io::Result<String> {
        let mut id = trace.id.clone();
        if id.is_empty() {
            id = format!("{}-{}", trace.made, sanitize(&trace.object));
        }
        let mut path = self.traces_dir().join(format!("{id}.trace"));
        let mut counter = 1;
        while path.exists() {
            path = self.traces_dir().join(format!("{id}-{counter}.trace"));
            counter += 1;
        }
        fs::write(&path, trace.render())?;
        Ok(path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string())
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        let path = self.traces_dir().join(format!("{id}.trace"));
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Обход среды: испарённые следы удаляются. Никого не спрашиваем — так устроен распад.
    pub fn sweep(&self, now: u64) -> Vec<String> {
        let mut gone = Vec::new();
        for trace in self.traces() {
            if trace.evaporated(now) {
                let _ = self.remove(&trace.id);
                gone.push(trace.id);
            }
        }
        gone
    }

    /// Что притягивает исполнителя со такой способностью.
    ///
    /// Потребность оживает, только если нет живого предупреждения на тот же предмет
    /// и никто не взял его прямо сейчас. Ни очереди, ни выдачи: только среда.
    pub fn attracting(&self, capability: &str, now: u64) -> Vec<Trace> {
        let traces = self.traces();
        let mut out: Vec<Trace> = Vec::new();
        for trace in &traces {
            if trace.kind != Kind::Need || trace.need != capability || trace.evaporated(now) {
                continue;
            }
            if blocked(&traces, &trace.object, now) {
                continue;
            }
            out.push(trace.clone());
        }
        out.sort_by(|a, b| {
            b.urgency(now)
                .partial_cmp(&a.urgency(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Самый сильный след, который притягивает исполнителя.
    pub fn strongest(&self, capability: &str, now: u64) -> Option<Trace> {
        self.attracting(capability, now).into_iter().next()
    }
}

/// Живое предупреждение или свежий захват на этот предмет.
pub fn blocked(traces: &[Trace], object: &str, now: u64) -> bool {
    traces.iter().any(|trace| {
        trace.object == object
            && matches!(trace.kind, Kind::Warn | Kind::Claim)
            && !trace.evaporated(now)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("stig-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Env::open(dir).unwrap()
    }

    #[test]
    fn need_attracts_only_the_matching_capability() {
        let env = env("cap");
        env.leave(&Trace::new(
            Kind::Need,
            "rust",
            "разобрать RFC 791",
            "owner",
            1000,
        ))
        .unwrap();
        assert_eq!(env.attracting("rust", 1000).len(), 1);
        assert!(
            env.attracting("python", 1000).is_empty(),
            "чужая способность не притягивается"
        );
    }

    #[test]
    fn strength_fades_and_the_need_stops_attracting_by_itself() {
        let env = env("fade");
        env.leave(&Trace::new(
            Kind::Need,
            "rust",
            "старое дело",
            "owner",
            1000,
        ))
        .unwrap();
        let later = 1000 + 86_400; // сутки
        assert!(
            env.attracting("rust", later).is_empty(),
            "брошенная потребность гаснет без чьей-то воли"
        );
        let gone = env.sweep(later);
        assert_eq!(gone.len(), 1, "испарённый след удаляется обходом");
        assert!(env.traces().is_empty());
    }

    #[test]
    fn strongest_trace_wins() {
        let env = env("strongest");
        env.leave(&Trace::new(Kind::Need, "rust", "старая работа", "a", 1000))
            .unwrap();
        // след, оставленный позже, моложе и потому сильнее — это и есть приоритет
        env.leave(&Trace::new(Kind::Need, "rust", "свежая работа", "b", 5000))
            .unwrap();
        let picked = env.strongest("rust", 5000).unwrap();
        assert_eq!(picked.object, "свежая работа");
    }

    #[test]
    fn claim_is_short_lived_and_releases_the_work_by_itself() {
        let env = env("claim");
        env.leave(&Trace::new(Kind::Need, "rust", "дело", "owner", 1000))
            .unwrap();
        env.leave(&Trace::new(Kind::Claim, "rust", "дело", "worker-1", 1000))
            .unwrap();
        assert!(
            env.strongest("rust", 1005).is_none(),
            "кто-то взял — остальные не лезут"
        );
        // процесс умер, ничего не убирая: след испарился за минуту
        assert!(
            env.strongest("rust", 1000 + 130).is_some(),
            "аренда без владельца истекает сама"
        );
    }

    #[test]
    fn warning_repels_others_and_expires() {
        let env = env("warn");
        env.leave(&Trace::new(
            Kind::Need,
            "rust",
            "трудное дело",
            "owner",
            1000,
        ))
        .unwrap();
        env.leave(&Trace::new(
            Kind::Warn,
            "rust",
            "трудное дело",
            "worker-1",
            1000,
        ))
        .unwrap();
        assert!(
            env.strongest("rust", 1000).is_none(),
            "другие видят: не вышло"
        );
        assert!(
            env.strongest("rust", 1000 + 3600).is_some(),
            "через час предупреждение испаряется и дело снова притягивает"
        );
    }

    #[test]
    fn done_leaves_a_trace_for_audit() {
        let env = env("done");
        env.leave(&Trace::new(Kind::Need, "rust", "дело", "owner", 1000))
            .unwrap();
        env.leave(&Trace::new(Kind::Done, "rust", "дело", "worker-1", 1200))
            .unwrap();
        let done = env
            .traces()
            .into_iter()
            .find(|t| t.kind == Kind::Done)
            .expect("след работы остаётся");
        assert_eq!(done.by, "worker-1");
    }

    #[test]
    fn parse_roundtrip_keeps_the_trace() {
        let trace = Trace::new(Kind::Need, "rust", "разобрать RFC 791", "owner", 1700);
        let back = Trace::parse(&trace.render()).unwrap();
        assert_eq!(back.kind, trace.kind);
        assert_eq!(back.need, trace.need);
        assert_eq!(back.object, trace.object);
        assert!((back.strength - trace.strength).abs() < 1e-9);
        assert_eq!(back.made, trace.made);
    }
}
