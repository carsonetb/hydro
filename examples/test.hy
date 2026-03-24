class Test(x: Int) {
    fn cls_fn {
        x;
    }
}

fn main {
    var x = Test(3);
    print(x.x.to_string());
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
