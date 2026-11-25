/*
 * libopenmpt.h — compatible subset for ahmod (modplay-rs Rust engines)
 *
 * Same names and signatures as libopenmpt's C API, so code written for libopenmpt
 * compiles and links against ahmod unchanged. Supported formats: MOD, IT, S3M, XM.
 * Output level matches libopenmpt's default render settings.
 *
 * Differences from libopenmpt:
 *  - logfunc / errfunc / ctls are accepted and ignored; *error_message is set to NULL.
 *  - Songs always continue at their restart position when they end (the behaviour of
 *    repeat count -1); the read functions always return `count` for a valid module.
 *  - Interpolation is linear (libopenmpt's default is an 8-tap filter).
 *
 * BSD-3-Clause (pt2-clone / ft2-clone by Olav Sørensen, modplay-rs).
 */
#ifndef AHMOD_LIBOPENMPT_H
#define AHMOD_LIBOPENMPT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define OPENMPT_ERROR_OK                0
#define OPENMPT_ERROR_BASE              256
#define OPENMPT_ERROR_INVALID_ARGUMENT  ( OPENMPT_ERROR_BASE +  44 )
#define OPENMPT_ERROR_GENERAL           ( OPENMPT_ERROR_BASE + 101 )

typedef struct openmpt_module openmpt_module;

typedef void (*openmpt_log_func)( const char * message, void * user );
typedef int (*openmpt_error_func)( int error, void * user );

typedef struct openmpt_module_initial_ctl {
	const char * ctl;
	const char * value;
} openmpt_module_initial_ctl;

uint32_t openmpt_get_library_version( void );

openmpt_module * openmpt_module_create_from_memory2( const void * filedata, size_t filesize, openmpt_log_func logfunc, void * loguser, openmpt_error_func errfunc, void * erruser, int * error, const char * * error_message, const openmpt_module_initial_ctl * ctls );
openmpt_module * openmpt_module_create_from_memory( const void * filedata, size_t filesize, openmpt_log_func logfunc, void * loguser, const openmpt_module_initial_ctl * ctls );
void openmpt_module_destroy( openmpt_module * mod );

int openmpt_module_set_repeat_count( openmpt_module * mod, int32_t repeat_count );
int32_t openmpt_module_get_repeat_count( openmpt_module * mod );

size_t openmpt_module_read_float_stereo( openmpt_module * mod, int32_t samplerate, size_t count, float * left, float * right );
size_t openmpt_module_read_interleaved_float_stereo( openmpt_module * mod, int32_t samplerate, size_t count, float * interleaved_stereo );
size_t openmpt_module_read_stereo( openmpt_module * mod, int32_t samplerate, size_t count, int16_t * left, int16_t * right );
size_t openmpt_module_read_interleaved_stereo( openmpt_module * mod, int32_t samplerate, size_t count, int16_t * interleaved_stereo );

double openmpt_module_get_position_seconds( openmpt_module * mod );
int32_t openmpt_module_get_current_order( openmpt_module * mod );
int32_t openmpt_module_get_current_row( openmpt_module * mod );
int32_t openmpt_module_get_num_channels( openmpt_module * mod );
double openmpt_module_set_position_order_row( openmpt_module * mod, int32_t order, int32_t row );

/* ahmod extension: mute / unmute a channel (returns 1 on success) */
int ahmod_set_channel_mute( openmpt_module * mod, int32_t channel, int mute );

#ifdef __cplusplus
}
#endif

#endif /* AHMOD_LIBOPENMPT_H */
