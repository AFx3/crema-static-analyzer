static unsigned char G = 0;

__attribute__((noinline))
static void sink(unsigned char *p)
{
    volatile unsigned char v = *p;
    (void)v;
}

int main(void)
{
    unsigned char local = 1;
    sink(&G);
    sink(&local);
    return 0;
}
