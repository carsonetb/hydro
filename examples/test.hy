fn main() {
    var text = if 1.05 == 1.05 {
        eval "true";
    } else { eval "false"; };
    print(text);
}
