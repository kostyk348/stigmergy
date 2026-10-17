//! `stig` — командная строка среды. Ни выдачи задач, ни статусов: только следы.
//!
//! ```text
//! stig need   --env DIR --capability rust --object "разобрать RFC 791" [--now TS]
//! stig status --env DIR [--now TS]
//! stig work   --env DIR --capability rust --as worker-1 [--do "команда"] [--now TS]
//! stig demo
//! ```

use std::process::Command;

use stigmergy::checo::{simulate, Fate, Plan, Worker};
use stigmergy::consent::{Circle, Verb, Verdict};
use stigmergy::dial::{Meters, PAIN};
use stigmergy::env::{now, Env, Kind, Trace};

struct Opts {
    args: Vec<String>,
}

impl Opts {
    fn parse(args: &[String]) -> Opts {
        Opts {
            args: args.to_vec(),
        }
    }
    fn flag(&self, name: &str) -> Option<&str> {
        let spelled = format!("--{name}");
        self.args
            .iter()
            .position(|item| item.as_str() == spelled.as_str())
            .and_then(|index| self.args.get(index + 1))
            .map(String::as_str)
    }
    fn number(&self, name: &str) -> Option<u64> {
        self.flag(name).and_then(|value| value.parse().ok())
    }
    fn many(&self, name: &str) -> Vec<&str> {
        let spelled = format!("--{name}");
        self.args
            .iter()
            .enumerate()
            .filter(|(_, item)| item.as_str() == spelled.as_str())
            .filter_map(|(index, _)| self.args.get(index + 1).map(String::as_str))
            .collect()
    }
    fn has(&self, name: &str) -> bool {
        let spelled = format!("--{name}");
        self.args
            .iter()
            .any(|item| item.as_str() == spelled.as_str())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("need") => cmd_need(&args[1..]),
        Some("status") => cmd_status(&args[1..]),
        Some("work") => cmd_work(&args[1..]),
        Some("lad") => cmd_lad(&args[1..]),
        Some("dial") => cmd_dial(&args[1..], true),
        Some("dials") => cmd_dial(&args[1..], false),
        Some("checo") => cmd_checo(&args[1..]),
        Some("demo") => cmd_demo(),
        Some("--help") | Some("-h") | None => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("неизвестная команда: {other}\n");
            print_help();
            2
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "stig — координация следами в среде (стигмергия), без доски и без очереди

  need    --env DIR --capability X --object \"что нужно\" [--strength F]
  status  --env DIR [--all]
  work    --env DIR --capability X --as кто [--do \"команда\"] [--now TS]
  demo

Правила среды:
  * работа существует как потребность с силой; сила убывает: strength * exp(-decay * age)
  * исполнителя никто не назначает: его притягивает самый сильный след его способности
  * взял — оставил короткий след «занято»; умер — след испарился, работа снова свободна
  * не вышло — оставил предупреждение; оно отталкивает других и тоже испаряется
  * потребность, на которую не нашлось способности, гаснет сама — молча

Код выхода: 0 сделано, 1 нечего делать, 2 ошибка."
    );
}

fn cmd_need(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let Some(env_path) = opts.flag("env") else {
        eprintln!("нужен --env DIR");
        return 2;
    };
    let capability = opts.flag("capability").unwrap_or("*").to_string();
    let object = opts.flag("object").unwrap_or("без имени").to_string();
    let at = opts.number("now").unwrap_or_else(now);
    let env = match Env::open(env_path) {
        Ok(env) => env,
        Err(err) => {
            eprintln!("не открыть среду: {err}");
            return 2;
        }
    };
    let mut trace = Trace::new(Kind::Need, &capability, &object, "owner", at);
    if let Some(strength) = opts.flag("strength").and_then(|s| s.parse::<f64>().ok()) {
        trace.strength = strength;
    }
    match env.leave(&trace) {
        Ok(id) => {
            println!(
                "след оставлен: {id} (способность {capability}, сила {:.2})",
                trace.strength
            );
            0
        }
        Err(err) => {
            eprintln!("не записать след: {err}");
            2
        }
    }
}

fn cmd_status(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let Some(env_path) = opts.flag("env") else {
        eprintln!("нужен --env DIR");
        return 2;
    };
    let at = opts.number("now").unwrap_or_else(now);
    let env = match Env::open(env_path) {
        Ok(env) => env,
        Err(err) => {
            eprintln!("не открыть среду: {err}");
            return 2;
        }
    };
    let traces = env.traces();
    if traces.is_empty() {
        println!("среда пуста: ни следа");
        return 0;
    }
    println!(
        "{:<6} {:<10} {:<10} {:<34} {:<10} {}",
        "сила", "вид", "способность", "предмет", "кто", "возраст"
    );
    let mut shown = 0;
    for trace in &traces {
        let urgency = trace.urgency(at);
        if urgency < stigmergy::env::EVAPORATED && !opts.has("all") {
            continue;
        }
        shown += 1;
        println!(
            "{:<6.2} {:<10} {:<10} {:<34} {:<10} {}",
            urgency,
            trace.kind.as_str(),
            trace.need,
            truncate(&trace.object, 34),
            truncate(&trace.by, 10),
            human_age(at.saturating_sub(trace.made))
        );
    }
    if shown == 0 {
        println!("все следы испарились — среда пуста по существу");
    }
    0
}

fn cmd_work(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let (Some(env_path), Some(capability)) = (opts.flag("env"), opts.flag("capability")) else {
        eprintln!("нужны --env DIR и --capability X");
        return 2;
    };
    let worker = opts.flag("as").unwrap_or("worker").to_string();
    let at = opts.number("now").unwrap_or_else(now);
    let env = match Env::open(env_path) {
        Ok(env) => env,
        Err(err) => {
            eprintln!("не открыть среду: {err}");
            return 2;
        }
    };
    let Some(target) = env.strongest(capability, at) else {
        println!("{worker}: ничего не притягивает (способность {capability})");
        return 1;
    };
    println!(
        "{worker}: притянуло «{}» (сила {:.2})",
        target.object,
        target.urgency(at)
    );
    env.leave(&Trace::new(
        Kind::Claim,
        capability,
        &target.object,
        &worker,
        at,
    ))
    .ok();

    let outcome = match opts.flag("do") {
        Some(command) => run(command),
        None => Ok(()),
    };

    match outcome {
        Ok(()) => {
            // потребность исполнена: убираем её и оставляем след самой работы
            for trace in env.traces() {
                if trace.kind == Kind::Need && trace.object == target.object {
                    let _ = env.remove(&trace.id);
                }
            }
            env.leave(&Trace::new(
                Kind::Done,
                capability,
                &target.object,
                &worker,
                at,
            ))
            .ok();
            println!("{worker}: сделано, след работы оставлен");
            0
        }
        Err(err) => {
            env.leave(&Trace::new(
                Kind::Warn,
                capability,
                &target.object,
                &worker,
                at,
            ))
            .ok();
            println!("{worker}: не вышло ({err}); предупреждение оттолкнёт других");
            1
        }
    }
}

fn run(command: &str) -> Result<(), String> {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "код {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(err) => Err(format!("не запустить: {err}")),
    }
}

fn human_age(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}ч", secs / 3600)
    } else if secs >= 60 {
        format!("{}м", secs / 60)
    } else {
        format!("{secs}с")
    }
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

// --- лад: русский консенсус --------------------------------------------------------
fn cmd_lad(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let Some(env_path) = opts.flag("env") else {
        eprintln!("нужен --env DIR");
        return 2;
    };
    let circle = match Circle::open(env_path) {
        Ok(circle) => circle,
        Err(err) => {
            eprintln!("не открыть круг: {err}");
            return 2;
        }
    };
    let at = opts.number("now").unwrap_or_else(now);
    match args.first().map(String::as_str) {
        Some("open") => {
            let id = opts.flag("id").unwrap_or("q1");
            let text = opts.flag("text").unwrap_or("без вопроса");
            let members: Vec<&str> = opts
                .flag("members")
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .collect();
            if members.is_empty() {
                eprintln!("нужен --members \"а,б,в\"");
                return 2;
            }
            let window = opts.number("window").unwrap_or(3600);
            match circle.open_question(id, text, &members, window) {
                Ok(question) => {
                    println!(
                        "круг открыт: «{}», участников {}, срок {} ({})",
                        question.text,
                        question.members.len(),
                        human_age(window),
                        id
                    );
                    0
                }
                Err(err) => {
                    eprintln!("не открыть: {err}");
                    2
                }
            }
        }
        Some("say") => {
            let id = opts.flag("id").unwrap_or("q1");
            let who = opts.flag("who").unwrap_or("");
            let verb = match opts.flag("verb").unwrap_or("лад") {
                "лад" => Verb::Lad,
                "возражение" => Verb::Objection,
                other => {
                    eprintln!("неизвестное слово: {other} (лад или возражение)");
                    return 2;
                }
            };
            let Some(mut question) = circle.find(id) else {
                eprintln!("вопроса {id} нет в кругу");
                return 2;
            };
            match question.speak(who, verb, opts.flag("reason").unwrap_or(""), at) {
                Ok(()) => {
                    if let Err(err) = circle.put(&question) {
                        eprintln!("не записать слово: {err}");
                        return 2;
                    }
                    println!("{who}: {}", verb.as_str());
                    0
                }
                Err(why) => {
                    eprintln!("отказано: {why}");
                    1
                }
            }
        }
        _ => {
            let questions = match opts.flag("id") {
                Some(id) => circle.find(id).into_iter().collect(),
                None => circle.questions(),
            };
            if questions.is_empty() {
                println!("круг пуст");
                return 0;
            }
            for question in questions {
                println!("«{}» ({})", question.text, question.id);
                for statement in &question.statements {
                    println!(
                        "  {:<10} {:<12} {}",
                        statement.member,
                        statement.verb.as_str(),
                        statement.reason
                    );
                }
                match question.verdict(at) {
                    Verdict::Lad { answered, silent } => {
                        println!(
                            "  лад: сказали {}, промолчали {} — и они в ответе{}",
                            answered,
                            silent.len(),
                            if silent.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", silent.join(", "))
                            }
                        );
                    }
                    Verdict::Blocked { by, reason } => {
                        println!("  не лад: возражает {by} — «{reason}»");
                    }
                    Verdict::Pending { left, next } => {
                        println!(
                            "  идёт: осталось {}, следующий по кругу — {}",
                            human_age(left),
                            next.unwrap_or_else(|| "все сказали".to_string())
                        );
                    }
                }
            }
            0
        }
    }
}

// --- алгедОнический метр -----------------------------------------------------------
fn cmd_dial(args: &[String], set: bool) -> i32 {
    let opts = Opts::parse(args);
    let Some(env_path) = opts.flag("env") else {
        eprintln!("нужен --env DIR");
        return 2;
    };
    let meters = match Meters::open(env_path) {
        Ok(meters) => meters,
        Err(err) => {
            eprintln!("не открыть метры: {err}");
            return 2;
        }
    };
    let at = opts.number("now").unwrap_or_else(now);
    if set {
        let (Some(who), Some(value)) = (opts.flag("who"), opts.number("value")) else {
            eprintln!("нужны --who и --value 0..100");
            return 2;
        };
        match meters.set(who, value as f64, opts.flag("note").unwrap_or(""), at) {
            Ok(dial) => {
                println!(
                    "{who}: стрелка на {} {}{}",
                    dial.value,
                    if dial.is_pain(at) {
                        "— это боль"
                    } else {
                        ""
                    },
                    if dial.note.is_empty() {
                        String::new()
                    } else {
                        format!(" («{}»)", dial.note)
                    }
                );
                0
            }
            Err(err) => {
                eprintln!("не записать: {err}");
                2
            }
        }
    } else {
        let all = meters.all();
        if all.is_empty() {
            println!("ни одного показания: человеческого слова в системе нет");
            return 0;
        }
        for dial in all {
            let now_value = dial.current(at);
            println!(
                "{:<10} {:>5.0} ({:>4.0}) {}{}",
                dial.by,
                dial.value,
                now_value,
                if now_value >= PAIN { "БОЛЬ " } else { "" },
                dial.note
            );
        }
        0
    }
}

// --- ЧЕКО: «что если» --------------------------------------------------------------
fn cmd_checo(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let Some(env_path) = opts.flag("env") else {
        eprintln!("нужен --env DIR");
        return 2;
    };
    let env = match Env::open(env_path) {
        Ok(env) => env,
        Err(err) => {
            eprintln!("не открыть среду: {err}");
            return 2;
        }
    };
    let workers: Vec<Worker> = opts
        .many("worker")
        .into_iter()
        .filter_map(Worker::parse)
        .collect();
    if workers.is_empty() {
        eprintln!("нужен хотя бы один --worker способность:сколько:секунд");
        return 2;
    }
    let plan = Plan {
        workers,
        horizon: opts.number("horizon").unwrap_or(86_400),
        step: 60,
    };
    let at = opts.number("now").unwrap_or_else(now);
    let needs = env.traces();
    let outcomes = simulate(&needs, &plan, at);
    if outcomes.is_empty() {
        println!("в среде нечего моделировать");
        return 0;
    }
    let mut died = 0;
    for outcome in &outcomes {
        match outcome.fate {
            Fate::Done(finish) => println!(
                "  {:>8}  {} [{}]",
                human_age(finish.saturating_sub(at)),
                truncate(&outcome.object, 40),
                outcome.need
            ),
            Fate::Died(dead) => {
                died += 1;
                println!(
                    "  {:>8}  {} — умрёт молча, никто не возьмёт",
                    human_age(dead.saturating_sub(at)),
                    truncate(&outcome.object, 40)
                );
            }
            Fate::Alive => println!(
                "  {:>8}  {} — доживёт до конца горизонта",
                "—",
                truncate(&outcome.object, 40)
            ),
        }
    }
    println!(
        "\nплан: {} проверок, {} умрут молча{}",
        outcomes.len(),
        died,
        if died > 0 {
            " — это и есть то, что увидел бы Бир в ЧЕКО"
        } else {
            ""
        }
    );
    if died > 0 {
        1
    } else {
        0
    }
}

// --- демонстрация -----------------------------------------------------------------
fn cmd_demo() -> i32 {
    let mut checks = 0;
    let mut fails = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        checks += 1;
        if !ok {
            fails += 1;
        }
        println!("  {} {name} — {detail}", if ok { "✓" } else { "✗" });
    };

    let dir = std::env::temp_dir().join(format!("stig-demo-{}", now()));
    let _ = std::fs::remove_dir_all(&dir);
    let env = Env::open(&dir).expect("среда открывается");
    println!("среда: {}", dir.display());

    let t0 = 1_000_000u64;
    for (cap, object) in [
        ("rust", "разобрать RFC 791"),
        ("python", "прочесать музей"),
        ("docs", "написать README"),
    ] {
        env.leave(&Trace::new(Kind::Need, cap, object, "owner", t0))
            .unwrap();
    }
    let all = env.attracting("rust", t0).len() + env.attracting("python", t0).len();
    check(
        "потребности лежат в среде, а не в списке задач",
        all == 2,
        format!("притягивают rust+python: {all}"),
    );

    // rust-исполнитель берётся за дело и проваливается
    let target = env.strongest("rust", t0).expect("есть что взять");
    env.leave(&Trace::new(
        Kind::Claim,
        "rust",
        &target.object,
        "worker-1",
        t0,
    ))
    .unwrap();
    check(
        "взял работу — другие не лезут",
        env.strongest("rust", t0 + 5).is_none(),
        "свежий след «занято» гасит притяжение".to_string(),
    );
    env.leave(&Trace::new(
        Kind::Warn,
        "rust",
        &target.object,
        "worker-1",
        t0,
    ))
    .unwrap();
    check(
        "не вышло — предупреждение отталкивает",
        env.strongest("rust", t0 + 10).is_none(),
        "никто не повторяет чужую ошибку сразу".to_string(),
    );

    // второй исполнитель делает python-работу
    let py = env
        .strongest("python", t0 + 10)
        .expect("python притягивает");
    env.leave(&Trace::new(
        Kind::Done,
        "python",
        &py.object,
        "worker-2",
        t0 + 10,
    ))
    .unwrap();
    for trace in env.traces() {
        if trace.kind == Kind::Need && trace.object == py.object {
            env.remove(&trace.id).ok();
        }
    }
    check(
        "сделанное ушло из среды, остался только след работы",
        env.attracting("python", t0 + 10).is_empty()
            && env.traces().iter().any(|t| t.kind == Kind::Done),
        "аудит есть, очереди нет".to_string(),
    );

    // предупреждение испаряется само
    check(
        "предупреждение испаряется по распаду, без уборщика",
        env.strongest("rust", t0 + 1_800).is_some(),
        "через полчаса дело снова притягивает".to_string(),
    );

    // docs никто не умеет — потребность гаснет молча
    check(
        "у способности нет исполнителя — потребность ещё видна",
        env.strongest("docs", t0 + 1_800).is_some(),
        "лежит и ждёт".to_string(),
    );
    let gone = env.sweep(t0 + 86_400);
    check(
        "никто не взял — потребность умерла молча",
        env.strongest("docs", t0 + 86_400).is_none() && !gone.is_empty(),
        format!("испарено следов: {}, никто не узнал", gone.len()),
    );

    let left = env.traces();
    check(
        "после суток в среде остался только след сделанной работы",
        left.iter().all(|t| t.kind == Kind::Done),
        format!(
            "следов: {} ({})",
            left.len(),
            left.iter()
                .map(|t| t.kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );

    // --- лад: русский консенсус ---
    {
        use stigmergy::consent::Question;
        let circle = Circle::open(dir.join("lad")).unwrap();
        let mut q = Question::new(
            "q1",
            "делать ли релиз",
            &["анна", "борис", "вера"],
            t0,
            3600,
        );
        circle.put(&q).unwrap();
        check(
            "в кругу не перебивают",
            q.speak("вера", Verb::Lad, "", t0 + 1).is_err(),
            format!("сейчас говорит {}", q.next_speaker().unwrap_or("—")),
        );
        q.speak("анна", Verb::Lad, "", t0 + 2).unwrap();
        q.speak("борис", Verb::Objection, "нет тестов", t0 + 3)
            .unwrap();
        q.speak("вера", Verb::Lad, "", t0 + 4).unwrap();
        check(
            "возражение блокирует дело",
            matches!(q.verdict(t0 + 3600), Verdict::Blocked { .. }),
            "не большинство решает, а снятое возражение".to_string(),
        );
        check(
            "круг кончился — открыт второй",
            q.round() == 2 && q.next_speaker() == Some("анна"),
            "в нём возражение можно снять".to_string(),
        );
        q.speak("анна", Verb::Lad, "", t0 + 5).unwrap();
        q.speak("борис", Verb::Lad, "тесты появились", t0 + 6)
            .unwrap();
        q.speak("вера", Verb::Lad, "", t0 + 7).unwrap();
        check(
            "снятое возражение даёт лад",
            matches!(q.verdict(t0 + 3600), Verdict::Lad { .. }),
            "дело идёт".to_string(),
        );
    }

    // --- алгедОнический метр ---
    {
        let meters = Meters::open(dir.join("dial")).unwrap();
        meters.set("анна", 70.0, "не то делаем", t0).unwrap();
        meters.set("борис", 10.0, "", t0).unwrap();
        let worst = meters.worst(t0).unwrap();
        check(
            "метр берёт худшее, а не среднее",
            worst.by == "анна" && worst.is_pain(t0),
            "10 и 70 — это 70, а не 40".to_string(),
        );
        check(
            "показание стареет без подтверждения",
            meters.pain(t0 + 86_400).is_none(),
            "через сутки «было плохо» уже не факт".to_string(),
        );
    }

    // --- ЧЕКО: что если ---
    {
        let plan_env = Env::open(dir.join("plan")).unwrap();
        for (cap, object) in [
            ("rust", "дело А"),
            ("rust", "дело Б"),
            ("docs", "документация"),
        ] {
            plan_env
                .leave(&Trace::new(Kind::Need, cap, object, "owner", t0))
                .unwrap();
        }
        let plan = Plan {
            workers: vec![Worker {
                capability: "rust".to_string(),
                count: 1,
                cost: 600,
            }],
            horizon: 86_400,
            step: 60,
        };
        let outcomes = simulate(&plan_env.traces(), &plan, t0);
        let done: Vec<u64> = outcomes
            .iter()
            .filter_map(|o| match o.fate {
                Fate::Done(at) => Some(at - t0),
                _ => None,
            })
            .collect();
        check(
            "симулятор видит очередь: второе дело ждёт первого",
            done == vec![600, 1200],
            format!("сделано через {done:?} секунд"),
        );
        check(
            "симулятор показывает молчаливую смерть до запуска",
            outcomes
                .iter()
                .any(|o| matches!(o.fate, Fate::Died(_)) && o.object == "документация"),
            "Бир давал спорить с моделью, а не с людьми".to_string(),
        );
    }

    println!("\nитог: {checks} проверок, {fails} провалов");
    let _ = std::fs::remove_dir_all(&dir);
    if fails == 0 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn flags_are_parsed_from_argv_tail() {
        let opts = Opts::parse(&[
            "--env".to_string(),
            "/tmp/x".to_string(),
            "--capability".to_string(),
            "rust".to_string(),
        ]);
        assert_eq!(opts.flag("env"), Some("/tmp/x"));
        assert_eq!(opts.flag("capability"), Some("rust"));
        assert_eq!(opts.number("missing"), None);
    }
}
