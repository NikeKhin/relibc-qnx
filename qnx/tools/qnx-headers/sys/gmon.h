/* gprof's run-time interface, which relibc's QNX backend provides for -p and
 * -pg: profiled programs call these through gcrt1.o (mcrt1.o). */
#ifndef _RELIBC_QNX_SYS_GMON_H
#define _RELIBC_QNX_SYS_GMON_H

#ifdef __cplusplus
extern "C" {
#endif

void monstartup(unsigned long lowpc, unsigned long highpc);
void moncontrol(int mode);
void _mcleanup(void);

#ifdef __cplusplus
}
#endif
#endif /* _RELIBC_QNX_SYS_GMON_H */
