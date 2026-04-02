#include "bdwgc/include/gc.h"
#include "bdwgc/include/gc/gc.h"
#include <stdio.h>
#include <string.h>

typedef struct {
  int size;
  char *val;
} String;

typedef struct {
  size_t elem_size;
  size_t capacity;
  size_t length;
  void *data;
} Vector;

void print(String *val) { fprintf(stderr, "%s\n", val->val); }

String *Int__to_string(int val) {
  char *mem = GC_MALLOC_ATOMIC(48);
  sprintf(mem, "%d", val);
  String *struct_mem = GC_MALLOC(sizeof(String));
  struct_mem->size = 48;
  struct_mem->val = mem;
  return struct_mem;
}

String *String__copy(String *from) {
  char *raw_mem = GC_MALLOC(from->size);
  memcpy(raw_mem, from->val, from->size);
  String *out = GC_MALLOC(sizeof(String));
  out->size = from->size;
  out->val = raw_mem;
  return out;
}

const char *String__to_cstr(String *from) { return from->val; }

Vector *Vector__new(size_t type_size, size_t capacity) {
  Vector *out = GC_MALLOC(sizeof(Vector));
  void *data = GC_MALLOC(type_size * capacity);
  out->elem_size = type_size;
  out->capacity = capacity;
  out->length = 0;
  out->data = data;
  return out;
}

void Vector__push(Vector *vec, void *item) {
  if (vec->length == vec->capacity) {
    vec->capacity *= 2;
    vec->data = GC_REALLOC(vec->data, vec->capacity * vec->elem_size);
  }

  void *dest = (char *)vec->data + (vec->length * vec->elem_size);
  memcpy(dest, item, vec->elem_size);
  vec->length += 1;
}

void Vector__push__Int(Vector *vec, int item) { Vector__push(vec, &item); }

void Vector__push__Bool(Vector *vec, char item) { Vector__push(vec, &item); }

void *Vector__get(Vector *vec, int index) {
  if (index >= vec->length) {
    fprintf(stderr, "Tried to access Vector element greater than its size.\n");
    return NULL;
  }

  return (char *)vec->data + (index * vec->elem_size);
}

int Vector__get__Int(Vector *vec, int index) {
  return *(int *)Vector__get(vec, index);
}

char Vector__get__Bool(Vector *vec, int index) {
  return *(char *)Vector__get(vec, index);
}

int Vector__len(Vector *vec) { return vec->length; }
