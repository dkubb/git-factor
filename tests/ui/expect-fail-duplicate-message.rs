use expect_fail_macro::expect_fail;

#[expect_fail(message = "a", message = "b")]
fn sample() {
    panic!("a");
}

fn main() {}
