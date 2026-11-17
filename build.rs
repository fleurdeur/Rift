fn main() {
    slint_build::compile("ui/app.slint").expect("Slint UI failed to compile");
    println!("cargo:rerun-if-changed=ui");
}
