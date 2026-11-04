//! Integration tests against real database servers (dev/compose.test.yml).
//!
//! ```text
//! dev/it-setup.sh
//! docker compose -f dev/compose.test.yml up -d --wait
//! set -a && . dev/.env && set +a
//! cargo test -p catable-core --features it
//! ```
//!
//! Every server is opt-in through its `CATABLE_IT_*` variable. When one is
//! missing, its tests are reported as `ignored` and the reason is printed
//! before the run — they never pass silently. SQLite needs no server and
//! always runs against a database built from dev/seed/sqlite.sql.

/// Returns `Err(message)` from the enclosing check when `cond` is false.
macro_rules! ensure {
    ($cond:expr, $($fmt:tt)+) => {
        if !$cond {
            return Err(format!($($fmt)+));
        }
    };
}

mod engines;
mod env;
mod session;
mod sqlite;
mod ssh;

use std::future::Future;

use libtest_mimic::{Arguments, Failed, Trial};

/// Outcome of one check; `Err` carries the failure message.
pub type Check = Result<(), String>;

fn main() {
    let args = Arguments::from_args();
    let mut targets = env::Targets::from_env();
    if !args.list {
        targets.sqlite = sqlite::seeded_database();
        targets.print_summary();
        for issue in ssh::KNOWN_ISSUES {
            eprintln!("known issue: {issue}\n");
        }
    }

    let mut trials = Vec::new();
    trials.extend(engines::trials("postgres", &targets.pg, true));
    trials.extend(engines::trials("mysql", &targets.mysql, true));
    trials.extend(engines::trials("mariadb", &targets.mariadb, true));
    trials.extend(engines::trials("sqlite", &targets.sqlite, false));
    trials.extend(session::trials("postgres", &targets.pg, true));
    trials.extend(session::trials("mysql", &targets.mysql, true));
    trials.extend(session::trials("mariadb", &targets.mariadb, true));
    trials.extend(session::trials("sqlite", &targets.sqlite, false));
    trials.extend(ssh::trials(&targets));

    let conclusion = libtest_mimic::run(&args, trials);
    sqlite::remove_database(&targets.sqlite);
    conclusion.exit();
}

/// Builds an async trial bound to `target`. An unavailable target yields an
/// ignored trial; forcing it with `--include-ignored` fails with the skip
/// reason instead of passing.
pub fn trial<T, F, Fut>(name: impl Into<String>, target: &env::Target<T>, check: F) -> Trial
where
    T: Clone + Send + 'static,
    F: FnOnce(T) -> Fut + Send + 'static,
    Fut: Future<Output = Check>,
{
    match target {
        Ok(target) => {
            let target = target.clone();
            Trial::test(name, move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(check(target)).map_err(Failed::from)
            })
        }
        Err(reason) => {
            let reason = format!("skipped: {reason}");
            Trial::test(name, move || Err(reason.into())).with_ignored_flag(true)
        }
    }
}
