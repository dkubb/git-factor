use expect_fail_macro::expect_fail;

fn msg() -> &'static str {
    "boom"
}

#[expect_fail(message = msg())]
fn sample() {
    panic!("boom");
}

fn main() {}
