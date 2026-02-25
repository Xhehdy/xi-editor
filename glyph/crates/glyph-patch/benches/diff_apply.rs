use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use glyph_patch::diff;

fn build_fixture(lines: usize) -> (String, String) {
    let mut old = String::with_capacity(lines * 64);
    for i in 0..lines {
        old.push_str(&format!("fn item_{i}() {{ let value = {i}; }}\n"));
    }

    let mut new = old.clone();

    // Mid-file replacement.
    let midpoint = lines / 2;
    let needle = format!("fn item_{midpoint}() {{ let value = {midpoint}; }}\n");
    let replacement =
        format!("fn item_{midpoint}() {{ let value = {midpoint}; log::info!(\"changed\"); }}\n");
    new = new.replacen(&needle, &replacement, 1);

    // Tail insertion.
    new.push_str("fn appended_tail() { println!(\"tail\"); }\n");

    // Head deletion (bounded).
    if let Some(head_newline) = new.find('\n') {
        new.replace_range(..=head_newline, "");
    }

    (old, new)
}

fn fixtures() -> Vec<(&'static str, String, String)> {
    vec![
        {
            let (old, new) = build_fixture(1_000);
            ("64kb-ish", old, new)
        },
        {
            let (old, new) = build_fixture(8_000);
            ("512kb-ish", old, new)
        },
        {
            let (old, new) = build_fixture(16_000);
            ("1mb-ish", old, new)
        },
    ]
}

fn bench_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_compute");
    for (name, old, new) in fixtures() {
        group.throughput(Throughput::Bytes(old.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("line_hash_diff", name),
            &(old, new),
            |b, (old, new)| {
                b.iter(|| {
                    let patch = diff(black_box(old), black_box(new));
                    black_box(patch);
                });
            },
        );
    }
    group.finish();
}

fn bench_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("patch_apply");
    for (name, old, new) in fixtures() {
        let patch = diff(&old, &new);
        group.throughput(Throughput::Bytes(old.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("apply_patch", name),
            &(old, patch),
            |b, (old, patch)| {
                b.iter(|| {
                    let updated = patch
                        .apply(black_box(old))
                        .expect("patch apply should succeed");
                    black_box(updated);
                });
            },
        );
    }
    group.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_patch_pipeline");
    for (name, old, new) in fixtures() {
        group.throughput(Throughput::Bytes(old.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("core_plus_ui_sim", name),
            &(old, new),
            |b, (old, new)| {
                b.iter(|| {
                    let core_start = Instant::now();
                    let patch = diff(black_box(old), black_box(new));
                    let core_response_micros = core_start.elapsed().as_micros() as u64;

                    let patch_bytes = rmp_serde::to_vec(&patch)
                        .map(|bytes| bytes.len() as u64)
                        .expect("patch serialization should succeed");

                    let ui_start = Instant::now();
                    let applied = patch
                        .apply(black_box(old))
                        .expect("simulated ui patch apply should succeed");
                    let ui_apply_micros = ui_start.elapsed().as_micros() as u64;

                    black_box((core_response_micros, patch_bytes, ui_apply_micros));
                    black_box(applied);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_diff, bench_apply, bench_end_to_end);
criterion_main!(benches);
