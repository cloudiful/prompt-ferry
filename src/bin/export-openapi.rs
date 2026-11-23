use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("expected output path"))?,
    );
    anyhow::ensure!(args.next().is_none(), "expected exactly one output path");
    prompt_ferry::openapi_export::export_standard_openapi(&output)
}
