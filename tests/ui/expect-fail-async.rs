use expect_fail_macro::expect_fail;

#[expect_fail(message = "boom")]
async fn sample() {}

fn main() {}
