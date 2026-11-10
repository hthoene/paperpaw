use paperpaw_sample_static_declarations::{battery, submit, widgets};

fn main() {
    for (name, batches) in [
        ("battery", battery::DECLARATIONS),
        ("widgets", widgets::DECLARATIONS),
    ] {
        let mut bytes = 0;
        submit(batches, |batch| {
            bytes += batch.len();
            Ok(())
        })
        .unwrap();
        println!(
            "{name}: {} image batches, {bytes} wire bytes",
            batches.len()
        );
    }
}
