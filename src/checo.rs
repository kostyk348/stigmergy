//! ЧЕКО: симулятор «что если», которым Бир давал спорить с моделью, а не с людьми.
//!
//! В чилийском проекте это был компонент Checo: руководитель мог прогнать решение через
//! модель и посмотреть последствия **до** того, как оно случится. Модель в те годы так
//! и не заработала — Бир честно писал, что она «работала визуально», а вычислитель
//! остался опытным. Здесь наоборот: модель простая, но считается по-настоящему.
//!
//! Что моделируется: потребности среды, их распад и исполнители со способностями.
//! Чего не моделируется (и это надо помнить): захваты, предупреждения, гонки,
//! усталость, разные скорости у разных исполнителей. Ответ поэтому — не пророчество,
//! а проверка: успеет ли план вообще, или потребность умрёт молча.

use crate::env::{Trace, EVAPORATED};

#[derive(Debug, Clone)]
pub struct Worker {
    pub capability: String,
    /// Сколько таких исполнителей.
    pub count: u32,
    /// Сколько секунд занимает одно дело.
    pub cost: u64,
}

impl Worker {
    /// `rust:2:600` — двое, по десять минут на дело.
    pub fn parse(spec: &str) -> Option<Worker> {
        let parts: Vec<&str> = spec.split(':').collect();
        if parts.len() != 3 {
            return None;
        }
        Some(Worker {
            capability: parts[0].to_string(),
            count: parts[1].parse().ok()?,
            cost: parts[2].parse().ok()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub workers: Vec<Worker>,
    pub horizon: u64,
    pub step: u64,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            workers: Vec::new(),
            horizon: 86_400,
            step: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fate {
    /// Сделано в такое-то время.
    Done(u64),
    /// Испарилось само: никто не взял до распада.
    Died(u64),
    /// Дожило до конца горизонта и осталось.
    Alive,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub object: String,
    pub need: String,
    pub fate: Fate,
    /// Сколько раз дело ждало в очереди к исполнителю.
    pub waited: u64,
}

/// Момент, когда след испарится, если его никто не возьмёт (аналитически).
pub fn death_time(trace: &Trace) -> u64 {
    if trace.decay <= 0.0 {
        return u64::MAX;
    }
    let age = (trace.strength / EVAPORATED).ln() / trace.decay;
    trace.made + age.ceil() as u64
}

/// Прогнать план по среде. Детерминированно: тот же вход — тот же ответ.
pub fn simulate(needs: &[Trace], plan: &Plan, start: u64) -> Vec<Outcome> {
    let mut pending: Vec<(usize, u64)> = needs
        .iter()
        .enumerate()
        .filter(|(_, trace)| trace.kind == crate::env::Kind::Need)
        .map(|(index, _)| (index, 0u64)) // (индекс, когда освободится)
        .collect();
    let mut outcomes: Vec<Outcome> = Vec::new();
    let mut taken: Vec<bool> = vec![false; needs.len()];
    let mut free_at: Vec<Vec<u64>> = plan
        .workers
        .iter()
        .map(|worker| vec![start; worker.count as usize])
        .collect();

    let mut t = start;
    while t <= start + plan.horizon {
        // кто свободен прямо сейчас
        for (worker_index, worker) in plan.workers.iter().enumerate() {
            for slot in 0..free_at[worker_index].len() {
                if free_at[worker_index][slot] > t {
                    continue;
                }
                // самый сильный живой след этой способности
                let mut best: Option<(usize, f64)> = None;
                for (index, trace) in needs.iter().enumerate() {
                    if taken[index]
                        || trace.kind != crate::env::Kind::Need
                        || trace.need != worker.capability
                    {
                        continue;
                    }
                    let urgency = trace.urgency(t);
                    if urgency < EVAPORATED {
                        continue;
                    }
                    if best.map(|(_, value)| urgency > value).unwrap_or(true) {
                        best = Some((index, urgency));
                    }
                }
                if let Some((index, _)) = best {
                    taken[index] = true;
                    let finish = t + worker.cost;
                    free_at[worker_index][slot] = finish;
                    outcomes.push(Outcome {
                        object: needs[index].object.clone(),
                        need: needs[index].need.clone(),
                        fate: Fate::Done(finish),
                        waited: t.saturating_sub(start),
                    });
                    let _ = &mut pending;
                }
            }
        }
        t += plan.step;
    }

    // что не взяли: умерло по распаду или дожило
    for (index, trace) in needs.iter().enumerate() {
        if taken[index] || trace.kind != crate::env::Kind::Need {
            continue;
        }
        let death = death_time(trace);
        let fate = if death <= start + plan.horizon {
            Fate::Died(death)
        } else {
            Fate::Alive
        };
        outcomes.push(Outcome {
            object: trace.object.clone(),
            need: trace.need.clone(),
            fate,
            waited: 0,
        });
    }

    outcomes.sort_by(|a, b| a.object.cmp(&b.object));
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{Kind, Trace};

    fn need(capability: &str, object: &str, made: u64) -> Trace {
        Trace::new(Kind::Need, capability, object, "owner", made)
    }

    #[test]
    fn two_workers_finish_two_jobs_in_one_round() {
        let needs = vec![need("rust", "a", 0), need("rust", "b", 0)];
        let plan = Plan {
            workers: vec![Worker {
                capability: "rust".to_string(),
                count: 2,
                cost: 600,
            }],
            horizon: 86_400,
            step: 60,
        };
        let out = simulate(&needs, &plan, 0);
        assert_eq!(out.len(), 2);
        assert!(
            out.iter().all(|o| matches!(o.fate, Fate::Done(600))),
            "{out:?}"
        );
    }

    #[test]
    fn one_worker_does_them_one_after_another() {
        let needs = vec![need("rust", "a", 0), need("rust", "b", 0)];
        let plan = Plan {
            workers: vec![Worker {
                capability: "rust".to_string(),
                count: 1,
                cost: 600,
            }],
            horizon: 86_400,
            step: 60,
        };
        let out = simulate(&needs, &plan, 0);
        let times: Vec<u64> = out
            .iter()
            .filter_map(|o| match o.fate {
                Fate::Done(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(times, vec![600, 1200], "второе дело ждёт первого: {out:?}");
    }

    #[test]
    fn a_need_nobody_can_take_dies_silently() {
        let needs = vec![need("docs", "написать README", 0)];
        let plan = Plan {
            workers: vec![Worker {
                capability: "rust".to_string(),
                count: 3,
                cost: 60,
            }],
            horizon: 86_400,
            step: 60,
        };
        let out = simulate(&needs, &plan, 0);
        match &out[0].fate {
            Fate::Died(at) => assert!(*at < 86_400, "умерло в {at}"),
            other => panic!("ожидалась молчаливая смерть, получено {other:?}"),
        }
    }

    #[test]
    fn slow_worker_can_miss_the_window_and_the_job_dies() {
        // дело живёт ~сутки (decay по умолчанию), а исполнитель занят 3 суток
        let needs = vec![need("rust", "долгое дело", 0)];
        let plan = Plan {
            workers: vec![Worker {
                capability: "rust".to_string(),
                count: 1,
                cost: 300_000,
            }],
            horizon: 1_000,
            step: 60,
        };
        let out = simulate(&needs, &plan, 100_000);
        match &out[0].fate {
            Fate::Done(_) => panic!("успел, а не должен был"),
            Fate::Died(_) | Fate::Alive => {}
        }
    }

    #[test]
    fn spec_is_parsed_from_the_command_line() {
        let worker = Worker::parse("rust:2:600").unwrap();
        assert_eq!(worker.capability, "rust");
        assert_eq!(worker.count, 2);
        assert_eq!(worker.cost, 600);
        assert!(Worker::parse("rust:600").is_none());
    }
}
