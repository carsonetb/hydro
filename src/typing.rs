pub trait Type: Hash {
    fn name(&self) -> String;
    fn generics(&self) -> Vec<dyn Type>;

    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name().hash(state);
        for generic in self.generics() {
            generic.hash()
        }
    }
}
