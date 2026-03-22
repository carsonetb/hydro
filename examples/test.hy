

fn main {
    var array = [1, 2, 3];
    array.push(5);
    if false {
        print("hello");
    }
    elif array[0] == 1 {
        print("hola");
    }
    else {
        print("nevermind");
    }
    print(array[3].to_string());
}

fn test(x: Int) -> Int {
    return x + 1;
}
