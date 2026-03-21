#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  int size;
  char *val;
} String;

void print(String *val) { printf("%s", val->val); }

String *Int__to_string(int val) {
  char *mem = malloc(48);
  sprintf(mem, "%d", val);
  String *struct_mem = malloc(sizeof(String));
  struct_mem->size = 48;
  struct_mem->val = mem;
  return struct_mem;
}

String *String__copy(String *from) {
  char *raw_mem = malloc(from->size);
  memcpy(raw_mem, from->val, from->size);
  String *out = malloc(sizeof(String));
  out->size = from->size;
  out->val = raw_mem;
  return out;
}
