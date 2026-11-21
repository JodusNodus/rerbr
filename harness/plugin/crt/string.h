/* See stdio.h. */
#ifndef RBR_CRT_STRING_H
#define RBR_CRT_STRING_H
#include "crt.h"
RBR_CRT void* __cdecl memcpy(void* d, const void* s, size_t n);
RBR_CRT void* __cdecl memmove(void* d, const void* s, size_t n);
RBR_CRT void* __cdecl memset(void* d, int c, size_t n);
RBR_CRT int __cdecl memcmp(const void* a, const void* b, size_t n);
RBR_CRT size_t __cdecl strlen(const char* s);
RBR_CRT int __cdecl strcmp(const char* a, const char* b);
RBR_CRT int __cdecl strncmp(const char* a, const char* b, size_t n);
RBR_CRT char* __cdecl strcpy(char* d, const char* s);
RBR_CRT char* __cdecl strncpy(char* d, const char* s, size_t n);
RBR_CRT char* __cdecl strcat(char* d, const char* s);
RBR_CRT char* __cdecl strchr(const char* s, int c);
RBR_CRT char* __cdecl strrchr(const char* s, int c);
RBR_CRT char* __cdecl strstr(const char* s, const char* t);
RBR_CRT size_t __cdecl strcspn(const char* s, const char* reject);
RBR_CRT char* __cdecl strtok(char* s, const char* delim);
RBR_CRT char* __cdecl _strlwr(char* s);
RBR_CRT int __cdecl _stricmp(const char* a, const char* b);
RBR_CRT int __cdecl _strnicmp(const char* a, const char* b, size_t n);
#endif
