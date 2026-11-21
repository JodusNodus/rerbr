/* See stdio.h. */
#ifndef RBR_CRT_STDLIB_H
#define RBR_CRT_STDLIB_H
#include "crt.h"
RBR_CRT void* __cdecl malloc(size_t n);
RBR_CRT void* __cdecl calloc(size_t n, size_t size);
RBR_CRT void* __cdecl realloc(void* p, size_t n);
RBR_CRT void __cdecl free(void* p);
RBR_CRT void __cdecl exit(int code);
RBR_CRT int __cdecl atoi(const char* s);
RBR_CRT long __cdecl strtol(const char* s, char** end, int base);
RBR_CRT unsigned long __cdecl strtoul(const char* s, char** end, int base);
RBR_CRT double __cdecl atof(const char* s);
RBR_CRT void __cdecl qsort(void* base, size_t n, size_t size, int (__cdecl* cmp)(const void*, const void*));
#endif
