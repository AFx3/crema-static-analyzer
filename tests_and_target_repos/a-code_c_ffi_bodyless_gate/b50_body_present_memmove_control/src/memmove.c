#include <stddef.h>

void *memmove(void *destination, const void *source, size_t count) {
    unsigned char *dst = destination;
    const unsigned char *src = source;
    if (dst < src) {
        for (size_t i = 0; i < count; ++i) {
            dst[i] = src[i];
        }
    } else if (dst > src) {
        for (size_t i = count; i > 0; --i) {
            dst[i - 1] = src[i - 1];
        }
    }
    return destination;
}
