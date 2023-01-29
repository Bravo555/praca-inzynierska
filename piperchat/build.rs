fn main() {
    glib_build_tools::compile_resources(
        "resources/piperchat/",
        "resources/piperchat/resources.gresource.xml",
        "piperchat.gresource",
    );
}
