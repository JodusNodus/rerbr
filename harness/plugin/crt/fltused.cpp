/* The clang-cl build has no C runtime startup: the floating-point marker its objects reference. */
extern "C" int _fltused = 0;
