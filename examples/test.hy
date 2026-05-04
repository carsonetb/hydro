class Test {
    var x = 1

    fn add -> Int {
        return x + 1
    }
}

fn main() {
    var text = input("Name? ")
    print("Hi, %s".format([text]))
}
