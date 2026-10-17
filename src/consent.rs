//! Лад: русский консенсус вместо голосования.
//!
//! Не «большинство», а **согласие**: дело идёт, если против него никто не возразил.
//! Возражение — не «голос против», оно требует причины и блокирует дело до тех пор,
//! пока возражение не снято. Это и есть лад: не победа над меньшинством, а снятие
//! возражения.
//!
//! Три правила, которые отличают это от голосования:
//!
//! 1. **Воздержания нет.** Кто промолчал до срока — тот согласился и в ответе
//!    (круговая порука). В голосовании молчание ничего не значит; здесь оно значит
//!    «беру на себя вместе со всеми».
//! 2. **Круг, а не площадь.** Говорят по очереди, в порядке круга. Перебить нельзя:
//!    высказывание вне очереди отклоняется. Так слышно каждого, а не самого громкого.
//! 3. **Срок обязателен.** У круга есть час, когда молчание становится согласием.
//!    Без срока молчание — это не согласие, а забывчивость.
//!
//! Цена честно: молчание считается согласием, поэтому «не прочитал» и «согласен»
//! неотличимы. Это accepting the worst case — и это же то, за что схему критиковали
//! в общине: круговая порука прикрывает равнодушие.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::env::now;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    /// Согласие вслух.
    Lad,
    /// Возражение с причиной.
    Objection,
}

impl Verb {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verb::Lad => "лад",
            Verb::Objection => "возражение",
        }
    }
    fn parse(text: &str) -> Option<Verb> {
        match text {
            "лад" => Some(Verb::Lad),
            "возражение" => Some(Verb::Objection),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub member: String,
    pub verb: Verb,
    pub reason: String,
    pub at: u64,
}

#[derive(Debug, Clone)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub members: Vec<String>,
    pub opened_at: u64,
    pub deadline: u64,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Согласие: возражений нет и срок вышел. Все, кто молчал, — в ответе.
    Lad {
        answered: usize,
        silent: Vec<String>,
    },
    /// Срок вышел, но есть неснятое возражение.
    Blocked { by: String, reason: String },
    /// Время ещё идёт.
    Pending { left: u64, next: Option<String> },
}

impl Question {
    pub fn new(id: &str, text: &str, members: &[&str], opened_at: u64, window: u64) -> Question {
        Question {
            id: id.to_string(),
            text: text.to_string(),
            members: members.iter().map(|m| m.to_string()).collect(),
            opened_at,
            deadline: opened_at + window,
            statements: Vec::new(),
        }
    }

    pub fn spoke(&self, member: &str) -> bool {
        self.statements.iter().any(|s| s.member == member)
    }

    pub fn count(&self, member: &str) -> usize {
        self.statements
            .iter()
            .filter(|s| s.member == member)
            .count()
    }

    /// Номер круга. Круг кончился, когда все сказали хотя бы по разу: тогда открывается
    /// следующий, и в нём можно снять возражение — иначе «лад» был бы неотменяем.
    pub fn round(&self) -> usize {
        self.members
            .iter()
            .map(|member| self.count(member))
            .min()
            .unwrap_or(0)
            + 1
    }

    /// Кто говорит следующим в текущем кругу.
    pub fn next_speaker(&self) -> Option<&str> {
        let round = self.round();
        self.members
            .iter()
            .find(|member| self.count(member) < round)
            .map(String::as_str)
    }

    /// Высказаться. Вне очереди нельзя — это и есть круг.
    pub fn speak(&mut self, member: &str, verb: Verb, reason: &str, at: u64) -> Result<(), String> {
        if !self.members.iter().any(|m| m == member) {
            return Err(format!("{member} не в кругу"));
        }
        if self.count(member) >= self.round() {
            return Err(format!("{member} уже говорил в этом кругу"));
        }
        if let Some(next) = self.next_speaker() {
            if next != member {
                return Err(format!(
                    "сейчас говорит {next}: в кругу не перебивают (следующий — {next})"
                ));
            }
        }
        if verb == Verb::Objection && reason.trim().is_empty() {
            return Err("возражение без причины не принимается".to_string());
        }
        self.statements.push(Statement {
            member: member.to_string(),
            verb,
            reason: reason.to_string(),
            at,
        });
        Ok(())
    }

    pub fn verdict(&self, at: u64) -> Verdict {
        if at < self.deadline {
            return Verdict::Pending {
                left: self.deadline - at,
                next: self.next_speaker().map(str::to_string),
            };
        }
        // Возражение считается снятым, если тот же человек сказал «лад» ПОЗЖЕ по порядку,
        // а не по часам: три слова в одну секунду — обычное дело, время тут не арбитр.
        let last_objection = self
            .statements
            .iter()
            .enumerate()
            .rev()
            .find(|(_, statement)| statement.verb == Verb::Objection);
        if let Some((index, last)) = last_objection {
            let withdrawn = self
                .statements
                .iter()
                .skip(index + 1)
                .any(|s| s.member == last.member && s.verb == Verb::Lad);
            if !withdrawn {
                return Verdict::Blocked {
                    by: last.member.clone(),
                    reason: last.reason.clone(),
                };
            }
        }
        let silent: Vec<String> = self
            .members
            .iter()
            .filter(|m| !self.spoke(m))
            .cloned()
            .collect();
        Verdict::Lad {
            answered: self.statements.len(),
            silent,
        }
    }

    pub fn render(&self) -> String {
        let mut out = format!(
            "id={}\ntext={}\nmembers={}\nopened={}\ndeadline={}\n",
            self.id,
            self.text,
            self.members.join(","),
            self.opened_at,
            self.deadline
        );
        for s in &self.statements {
            out.push_str(&format!(
                "say={}|{}|{}|{}\n",
                s.member,
                s.verb.as_str(),
                s.at,
                s.reason
            ));
        }
        out
    }

    pub fn parse(text: &str) -> Option<Question> {
        let mut q = Question {
            id: String::new(),
            text: String::new(),
            members: Vec::new(),
            opened_at: 0,
            deadline: 0,
            statements: Vec::new(),
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "id" => q.id = value.trim().to_string(),
                "text" => q.text = value.trim().to_string(),
                "members" => {
                    q.members = value
                        .trim()
                        .split(',')
                        .filter(|m| !m.is_empty())
                        .map(str::to_string)
                        .collect()
                }
                "opened" => q.opened_at = value.trim().parse().unwrap_or(0),
                "deadline" => q.deadline = value.trim().parse().unwrap_or(0),
                "say" => {
                    let parts: Vec<&str> = value.split('|').collect();
                    if parts.len() >= 4 {
                        if let Some(verb) = Verb::parse(parts[1]) {
                            q.statements.push(Statement {
                                member: parts[0].to_string(),
                                verb,
                                at: parts[2].parse().unwrap_or(0),
                                reason: parts[3..].join("|"),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        Some(q)
    }
}

/// Круг хранится в среде рядом со следами — отдельного сервера нет.
pub struct Circle {
    root: PathBuf,
}

impl Circle {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Circle> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("circle"))?;
        Ok(Circle { root })
    }

    fn dir(&self) -> PathBuf {
        self.root.join("circle")
    }

    pub fn questions(&self) -> Vec<Question> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.dir()) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lad") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(question) = Question::parse(&text) {
                out.push(question);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn find(&self, id: &str) -> Option<Question> {
        self.questions().into_iter().find(|q| q.id == id)
    }

    pub fn put(&self, question: &Question) -> io::Result<()> {
        fs::write(
            self.dir().join(format!("{}.lad", question.id)),
            question.render(),
        )
    }

    pub fn open_question(
        &self,
        id: &str,
        text: &str,
        members: &[&str],
        window: u64,
    ) -> io::Result<Question> {
        let question = Question::new(id, text, members, now(), window);
        self.put(&question)?;
        Ok(question)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> Question {
        Question::new(
            "q1",
            "делать ли релиз",
            &["анна", "борис", "вера"],
            1000,
            3600,
        )
    }

    #[test]
    fn silence_is_consent_but_only_after_the_deadline() {
        let q = question();
        assert_eq!(
            q.verdict(1000 + 60),
            Verdict::Pending {
                left: 3540,
                next: Some("анна".to_string())
            }
        );
        match q.verdict(1000 + 3600) {
            Verdict::Lad { silent, answered } => {
                assert_eq!(answered, 0);
                assert_eq!(silent.len(), 3, "все молчали — все согласны и в ответе");
            }
            other => panic!("ожидался лад, получено {other:?}"),
        }
    }

    #[test]
    fn the_circle_forbids_interruption() {
        let mut q = question();
        let err = q.speak("вера", Verb::Lad, "", 1001).unwrap_err();
        assert!(err.contains("анна"), "говорит следующий по кругу: {err}");
        q.speak("анна", Verb::Lad, "", 1002).unwrap();
        q.speak("борис", Verb::Lad, "", 1003).unwrap();
        q.speak("вера", Verb::Lad, "", 1004).unwrap();
    }

    #[test]
    fn objection_blocks_and_needs_a_reason() {
        let mut q = question();
        assert!(q.speak("анна", Verb::Objection, "", 1001).is_err());
        q.speak("анна", Verb::Lad, "", 1001).unwrap();
        q.speak("борис", Verb::Objection, "нет тестов", 1002)
            .unwrap();
        match q.verdict(1000 + 3600) {
            Verdict::Blocked { by, reason } => {
                assert_eq!(by, "борис");
                assert_eq!(reason, "нет тестов");
            }
            other => panic!("ожидалась блокировка, получено {other:?}"),
        }
    }

    #[test]
    fn withdrawn_objection_returns_lad() {
        let mut q = question();
        q.speak("анна", Verb::Lad, "", 1001).unwrap();
        q.speak("борис", Verb::Objection, "нет тестов", 1002)
            .unwrap();
        q.speak("вера", Verb::Lad, "", 1003).unwrap();
        // возражение снимается на втором кругу: борис говорит «лад»
        q.statements.push(Statement {
            member: "борис".to_string(),
            verb: Verb::Lad,
            reason: "тесты появились".to_string(),
            at: 5000,
        });
        match q.verdict(6000) {
            Verdict::Lad { .. } => {}
            other => panic!("снятое возражение даёт лад, получено {other:?}"),
        }
    }

    #[test]
    fn question_survives_roundtrip() {
        let mut q = question();
        q.speak("анна", Verb::Objection, "нет тестов", 1001)
            .unwrap();
        let back = Question::parse(&q.render()).unwrap();
        assert_eq!(back.id, q.id);
        assert_eq!(back.members, q.members);
        assert_eq!(back.statements.len(), 1);
        assert_eq!(back.statements[0].verb, Verb::Objection);
        assert_eq!(back.statements[0].reason, "нет тестов");
    }
}
