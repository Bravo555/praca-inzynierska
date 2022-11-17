fn main() {
    glib_build_tools::compile_resources(
        "resources/",
        "resources/resources.gresource.xml",
        "todo_1.gresource",
    );
}
