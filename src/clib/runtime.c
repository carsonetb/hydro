#include "bdwgc/include/gc.h"

extern void lang_main();

int main(int argc, char **argv) {
  GC_INIT();

  lang_main();

  return 0;
}
