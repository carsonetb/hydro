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

// Not very efficient but doesn't really need to be.
String *input(String *val) {
  fprintf(stderr, "%s", val->val);
  char *line = NULL;
  size_t len = 0;
  ssize_t read = getline(&line, &len, stdin);
  String *out = GC_MALLOC(sizeof(String));
  char *mem = GC_MALLOC(out->size);
  strcat(mem, line);
  mem[strcspn(mem, "\n")] = '\0'; // Remove newline.

  out->size = strlen(mem);
  out->val = mem;

  return out;
}

String *String__from_cstr_nosize(char *from) {
  String *out = GC_MALLOC(sizeof(String));
  out->size = strlen(from);
  out->val = from;
  return out;
}

String *String__from_cstr(char *from, int size) {
  String *out = GC_MALLOC(sizeof(String));
  out->size = size;
  out->val = from;
  return out;
}

String *String__new(size_t size) {
  char *mem = GC_MALLOC(size);
  mem[0] = '\0';
  return String__from_cstr(mem, size);
}

String *Int__to_string(int val) {
  char *mem = GC_MALLOC_ATOMIC(48);
  sprintf(mem, "%d", val);
  return String__from_cstr(mem, 48);
}

String *Float__to_string(float val) {
  char *mem = GC_MALLOC_ATOMIC(48);
  sprintf(mem, "%f", val);
  return String__from_cstr(mem, 48);
}

String *String__copy(String *from) {
  char *raw_mem = GC_MALLOC(from->size);
  memcpy(raw_mem, from->val, from->size);
  return String__from_cstr(raw_mem, from->size);
}

String *String__concat(String *this, String *other) {
  String *dest = String__new(this->size + other->size);
  strcat(dest->val, this->val);
  strcat(other->val, this->val);
  return dest;
}

int String__eq(String* this, String* other) {
    return strcmp(this->val, other->val) == 0;
}

int String__neq(String* this, String* other) {
    return strcmp(this->val, other->val) != 0;
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

void *Vector__get(Vector *vec, int index) {
  if (index >= vec->length) {
    fprintf(stderr, "Tried to access Vector element greater than its size.\n");
    return NULL;
  }
  return (char *)vec->data + (index * vec->elem_size);
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

int Vector__get__Int(Vector *vec, int index) {
  return *(int *)Vector__get(vec, index);
}

char Vector__get__Bool(Vector *vec, int index) {
  return *(char *)Vector__get(vec, index);
}

int Vector__len(Vector *vec) { return vec->length; }

String *String__format(String *format, Vector *vals) {
  // TODO: This trusts that number of %s in string == vals->length.
  int res_size = format->size;
  for (int i = 0; i < vals->length; i++) {
    res_size += ((String *)Vector__get(vals, i))->size - 2;
  }

  char *result = GC_MALLOC(res_size + 1);
  char *dest = result;
  int i = 0;
  char *fmtcurrent = format->val;
  while (*fmtcurrent) {
    if (*fmtcurrent == '%' && *(fmtcurrent + 1) == 's' && i < vals->length) {
      char *this = ((String *)Vector__get(vals, i))->val;
      strcpy(dest, this);
      dest += strlen(this);
      i++;
      fmtcurrent += 2; // skip
    } else {
      *dest++ = *fmtcurrent++; // sped syntax
    }
  }
  *dest = '\0';

  return String__from_cstr(result, res_size + 1);
}
