
/* Provided by relibc's QNX backend, which implements it ahead of relibc. */
#ifndef _RELIBC_QNX_TIME_H
#define _RELIBC_QNX_TIME_H
#ifdef __cplusplus
extern "C" {
#endif

int clock_nanosleep(clockid_t clock_id, int flags, const struct timespec *rqtp,
                    struct timespec *rmtp);

#ifdef __cplusplus
}
#endif
#endif /* _RELIBC_QNX_TIME_H */
