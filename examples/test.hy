class Test(x: Int) {
    fn test {
        x;
    }
}

fn main {
    Test(1);
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
