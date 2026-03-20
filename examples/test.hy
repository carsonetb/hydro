

fn main {
    test2(105.to_string);
    var x = 1 == 1 || 2 > test(1);
}

fn test2(function: MemberFunction<Int, Tuple<>, String>) {
    print(function());
}


fn test(x: Int) -> Int {
    return x + 1;
}
