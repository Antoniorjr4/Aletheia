/* O relógio que o processo vê, adiantado ou atrasado: é a manipulação do relógio local do
 * executor (settimeofday, NTP forjado, VM pausada), feita por interposição de `clock_gettime`.
 * Só o relógio de parede (CLOCK_REALTIME) muda; o monotônico não. Usado por tests/rc4.rs. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <time.h>

int clock_gettime(clockid_t id, struct timespec *ts) {
  static int (*real)(clockid_t, struct timespec *);
  if (!real) real = dlsym(RTLD_NEXT, "clock_gettime");
  int r = real(id, ts);
  if (r == 0 && id == CLOCK_REALTIME) {
    const char *o = getenv("ATH_FAKE_CLOCK_OFFSET");
    if (o) ts->tv_sec += atol(o);
  }
  return r;
}
