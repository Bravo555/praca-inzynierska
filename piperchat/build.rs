fn main() {
    glib_build_tools::compile_resources(
        "resources/todo/",
        "resources/todo/resources.gresource.xml",
        "todo_1.gresource",
    );
    glib_build_tools::compile_resources(
        "resources/piperchat/",
        "resources/piperchat/resources.gresource.xml",
        "piperchat.gresource",
    );
}
