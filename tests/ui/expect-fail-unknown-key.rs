use expect_fail_macro::expect_fail;

#[expect_fail(unexpected = "boom")]
fn sample() {
    panic!("boom");
}

fn main() {}
