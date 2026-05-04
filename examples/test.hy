
fn main() {
    print(1.to_string());
    print("x" + "y");
    var text = if 1.05 == 1.04 {
        eval "true";
    } elif 1.05 == 1.05 {
        eval "false";
    } else {
        eval "none";
    };
    print(text);
}
