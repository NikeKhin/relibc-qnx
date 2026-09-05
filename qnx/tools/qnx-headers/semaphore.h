
/* Provided by relibc's QNX backend, which implements them ahead of relibc. */
#ifndef _RELIBC_QNX_SEMAPHORE_H
#define _RELIBC_QNX_SEMAPHORE_H
#ifdef __cplusplus
extern "C" {
#endif

#ifndef SEM_FAILED
#define SEM_FAILED ((sem_t *)0)
#endif

sem_t *sem_open(const char *name, int oflag, ...);
int sem_close(sem_t *sem);
int sem_unlink(const char *name);

#ifdef __cplusplus
}
#endif
#endif /* _RELIBC_QNX_SEMAPHORE_H */
