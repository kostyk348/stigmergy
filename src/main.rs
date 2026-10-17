//! `stig` — командная строка среды. Ни выдачи задач, ни статусов: только следы.
//!
//! ```text
//! stig need   --env DIR --capability rust --object "разобрать RFC 791" [--now TS]
//! stig status --env DIR [--now TS]
//! stig work   --env DIR --capability rust --as worker-1 [--do "команда"] [--now TS]
//! stig demo
//! ```

use std::process::Command;

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
