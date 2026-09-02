//! Klerq desktop entry point.
//!
//! Until the `egui` front-end lands (see PLAN.md, Phase 7), this binary runs a
//! text-mode demo that exercises every engine — proof the suite composes and a
//! smoke check for CI.

fn main() {
    print!("{}", klerq_desktop::run_demo());
}
