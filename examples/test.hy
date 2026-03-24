class Test(x: Int) {
    fn cls_fn -> Int {
        return x;
    }
}

fn main {
    var x = Test(3);
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
