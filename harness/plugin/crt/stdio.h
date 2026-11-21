/* The C runtime declarations the harness plugin uses, for the clang-cl build (build.sh without MSVC 7.1): the plugin
   links msvcrt.dll, the system C runtime, through an import library build.sh makes. */
#ifndef RBR_CRT_STDIO_H
#define RBR_CRT_STDIO_H
#include "crt.h"
typedef struct _iobuf FILE;
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define EOF (-1)
#define _IOFBF 0x0000
RBR_CRT FILE* __cdecl fopen(const char* name, const char* mode);
RBR_CRT int __cdecl fclose(FILE* f);
RBR_CRT int __cdecl fprintf(FILE* f, const char* fmt, ...);
RBR_CRT int __cdecl fscanf(FILE* f, const char* fmt, ...);
RBR_CRT size_t __cdecl fread(void* p, size_t size, size_t n, FILE* f);
RBR_CRT size_t __cdecl fwrite(const void* p, size_t size, size_t n, FILE* f);
RBR_CRT int __cdecl fseek(FILE* f, long off, int origin);
RBR_CRT long __cdecl ftell(FILE* f);
RBR_CRT int __cdecl fflush(FILE* f);
RBR_CRT char* __cdecl fgets(char* s, int n, FILE* f);
RBR_CRT int __cdecl setvbuf(FILE* f, char* buf, int mode, size_t size);
RBR_CRT int __cdecl sprintf(char* buf, const char* fmt, ...);
RBR_CRT int __cdecl sscanf(const char* s, const char* fmt, ...);
RBR_CRT int __cdecl _snprintf(char* buf, size_t n, const char* fmt, ...);
#endif
