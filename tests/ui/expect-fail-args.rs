use expect_fail_macro::expect_fail;

#[expect_fail(message = "boom")]
fn sample(value: i32) {
    let _ = value;
}

fn main() {}
