use expect_fail_macro::expect_fail;

#[expect_fail]
fn sample() {
    panic!("missing");
}

fn main() {}
