use expect_fail_macro::expect_fail;

#[expect_fail(message = 123)]
fn sample() {
    panic!("123");
}

fn main() {}
