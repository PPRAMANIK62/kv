//! The three minimizers on the same failure: seed 0 of the planted bug.
//! Run it with `cargo test --release -p kvkit race -- --ignored --nocapture`.

use std::time::Instant;

use crate::{
    experiments::{minimize_cut::minimize_cut, minimize_one::minimize_one, off_by_one::Planted},
    testkit::{Op, OpGen, check, minimize, scratch_dir},
};

/// Counts what a minimizer costs: test runs, and steps executed.
#[derive(Default)]
struct Cost {
    runs: u64,
    steps: u64,
}

impl Cost {
    /// Runs the model test on `ops`, counting it, and returns the failing
    /// step if there is one.
    fn check(&mut self, ops: &[Op]) -> Option<usize> {
        let dir = scratch_dir("steps_race");
        let result = check::<Planted>(&dir, ops).err().map(|f| f.step);
        self.runs += 1;
        self.steps += result.unwrap_or(ops.len()) as u64;
        result
    }
}

#[test]
#[ignore = "takes 54 seconds on my machine"]
fn race() {
    let ops = OpGen::default().ops(0, 5000);
    let row = |name: &str, runs: &str, steps: &str, seconds: &str, found: &str| {
        format!("{name:<12} {runs:>8} {steps:>11} {seconds:>9}  {found}\n")
    };
    // Built up and printed once, so other tests' output can't land inside.
    let mut table = row("minimizer", "runs", "steps run", "seconds", "found");
    // Each minimized sequence, written to paste into `regressions` in
    // tests/model.rs, with the minimizers that found it.
    let mut found: Vec<(String, Vec<&str>)> = Vec::new();
    for name in ["one", "cut", "chunks+cut"] {
        let mut cost = Cost::default();
        let start = Instant::now();
        let small = match name {
            "one" => minimize_one(&ops, |ops| cost.check(ops).is_some()),
            "cut" => minimize_cut(&ops, |ops| cost.check(ops)),
            _ => minimize(&ops, |ops| cost.check(ops)),
        };
        let seconds = start.elapsed().as_secs_f64();
        table += &row(
            name,
            &commas(cost.runs),
            &commas(cost.steps),
            &format!("{seconds:.3}"),
            &format!("{} ops", small.len()),
        );
        let ops: Vec<String> = small.iter().map(Op::to_string).collect();
        let line = format!("vec![{}],", ops.join(", "));
        match found.iter_mut().find(|(seen, _)| *seen == line) {
            Some((_, names)) => names.push(name),
            None => found.push((line, vec![name])),
        }
        assert_eq!(cost.check(&small), Some(small.len()));
    }
    table += "\n";
    let width = found.iter().map(|(line, _)| line.len()).max().unwrap_or(0);
    for (line, names) in &found {
        table += &format!("{line:<width$}  // {}\n", names.join(", "));
    }
    print!("{table}");
    let _ = std::fs::remove_dir_all(scratch_dir("steps_race"));
}

/// `n` with a comma between each group of three digits: 3,174,825.
fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// At which step the model test first sees the planted bug, for seeds 0
/// to 99. Run it with `cargo test --release -p kvkit how_soon -- --ignored
/// --nocapture`.
#[test]
#[ignore]
fn how_soon() {
    let dir = scratch_dir("steps_how_soon");
    let mut found = Vec::new();
    for seed in 0..100 {
        let ops = OpGen::default().ops(seed, 5000);
        if let Err(failure) = check::<Planted>(&dir, &ops) {
            found.push(failure.step);
        }
    }
    found.sort();
    println!(
        "{} of 100 seeds fail within 5,000 steps; first failing step: \
         min {}, median {}, max {}",
        found.len(),
        found[0],
        found[found.len() / 2],
        found[found.len() - 1]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
