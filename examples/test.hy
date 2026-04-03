import raylib;

class Test(x: Int) {
    var y = 2;
    fn cls_fn -> Int {
        return x;
    }
}

fn main {
    init_window(500, 500, "Test");
    while true {
    }
    var x = Test(3);
    x.y = x.y + 1;
    print(x.y.to_string());
    print(x.cls_fn().to_string());
    var array = [1, 2, 3];
    if false {
        print("hello");
    }
    elif array[0] == 1 {
        print("hola");
    }
    else {
        print("nevermind");
    }
    for item in array {
    }
}

fn test(x: Int) -> Int {
    return x + 1;
}
