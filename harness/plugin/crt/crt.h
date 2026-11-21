/* See stdio.h. */
#ifndef RBR_CRT_H
#define RBR_CRT_H
typedef unsigned int size_t;
#ifndef NULL
#define NULL 0
#endif
#ifdef __cplusplus
#define RBR_CRT extern "C" __declspec(dllimport)
#else
#define RBR_CRT extern __declspec(dllimport)
#endif
#endif
