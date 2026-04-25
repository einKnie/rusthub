fn main() {
    #[cfg(feature = "slint_gui")]
    slint_build::compile("src/ui/sensorui.slint").unwrap();
}
