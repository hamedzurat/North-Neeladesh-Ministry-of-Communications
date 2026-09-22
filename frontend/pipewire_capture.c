#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>
#include <spa/param/format-utils.h>
#include <spa/utils/result.h>

#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct NN_Pipewire_Capture {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    pthread_mutex_t mutex;
    int16_t *ring;
    uint32_t capacity;
    uint32_t start;
    uint32_t count;
    int failed;
} NN_Pipewire_Capture;

static void on_process(void *data) {
    NN_Pipewire_Capture *capture = data;
    struct pw_buffer *buffer = pw_stream_dequeue_buffer(capture->stream);
    if (buffer == NULL) {
        capture->failed = 1;
        return;
    }

    struct spa_data *source = &buffer->buffer->datas[0];
    if (source->data != NULL && source->chunk != NULL && source->chunk->size > 0) {
        const uint32_t samples = source->chunk->size / sizeof(int16_t);
        const int16_t *input = (const int16_t *)((uint8_t *)source->data + source->chunk->offset);
        pthread_mutex_lock(&capture->mutex);
        for (uint32_t index = 0; index < samples; index++) {
            if (capture->count < capture->capacity) {
                const uint32_t position = (capture->start + capture->count) % capture->capacity;
                capture->ring[position] = input[index];
                capture->count++;
            } else {
                capture->ring[capture->start] = input[index];
                capture->start = (capture->start + 1) % capture->capacity;
            }
        }
        pthread_mutex_unlock(&capture->mutex);
    }
    pw_stream_queue_buffer(capture->stream, buffer);
}

static const struct pw_stream_events STREAM_EVENTS = {
    PW_VERSION_STREAM_EVENTS,
    .process = on_process,
};

NN_Pipewire_Capture *nn_pw_capture_start(uint32_t rate, uint32_t channels, uint32_t max_samples) {
    if (channels != 1 || max_samples == 0) return NULL;
    NN_Pipewire_Capture *capture = calloc(1, sizeof(*capture));
    if (capture == NULL) return NULL;
    capture->capacity = max_samples;
    capture->ring = calloc(max_samples, sizeof(int16_t));
    if (capture->ring == NULL || pthread_mutex_init(&capture->mutex, NULL) != 0) {
        free(capture->ring);
        free(capture);
        return NULL;
    }

    pw_init(NULL, NULL);
    capture->loop = pw_thread_loop_new("north-neeladesh-capture", NULL);
    if (capture->loop == NULL) goto fail;
    struct pw_context *context = pw_context_new(pw_thread_loop_get_loop(capture->loop), NULL, 0);
    if (context == NULL) goto fail;
    struct pw_core *core = pw_context_connect(context, NULL, 0);
    if (core == NULL) goto fail;
    struct pw_properties *properties = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Audio",
        PW_KEY_MEDIA_CATEGORY, "Capture",
        PW_KEY_MEDIA_ROLE, "Production",
        NULL);
    capture->stream = pw_stream_new_simple(
        pw_thread_loop_get_loop(capture->loop),
        "North Neeladesh microphone",
        properties,
        &STREAM_EVENTS,
        capture);
    if (capture->stream == NULL) goto fail;

    uint8_t pod_buffer[1024];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(pod_buffer, sizeof(pod_buffer));
    const struct spa_pod *params[1];
    params[0] = spa_format_audio_raw_build(
        &builder, SPA_PARAM_EnumFormat,
        &(struct spa_audio_info_raw){
            .format = SPA_AUDIO_FORMAT_S16,
            .rate = rate,
            .channels = channels,
        });
    if (pw_stream_connect(capture->stream, PW_DIRECTION_INPUT, PW_ID_ANY,
                          PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS | PW_STREAM_FLAG_RT_PROCESS,
                          params, 1) < 0) goto fail;
    if (pw_thread_loop_start(capture->loop) < 0) goto fail;
    (void)core;
    return capture;

fail:
    if (capture->stream) pw_stream_destroy(capture->stream);
    if (capture->loop) pw_thread_loop_destroy(capture->loop);
    pthread_mutex_destroy(&capture->mutex);
    free(capture->ring);
    free(capture);
    return NULL;
}

void nn_pw_capture_begin(NN_Pipewire_Capture *capture) {
    if (capture == NULL) return;
    pthread_mutex_lock(&capture->mutex);
    capture->start = 0;
    capture->count = 0;
    pthread_mutex_unlock(&capture->mutex);
}

uint32_t nn_pw_capture_read(NN_Pipewire_Capture *capture, int16_t *output, uint32_t capacity) {
    if (capture == NULL || output == NULL || capacity == 0) return 0;
    pthread_mutex_lock(&capture->mutex);
    const uint32_t count = capture->count < capacity ? capture->count : capacity;
    const uint32_t skip = capture->count - count;
    for (uint32_t index = 0; index < count; index++) {
        output[index] = capture->ring[(capture->start + skip + index) % capture->capacity];
    }
    capture->start = 0;
    capture->count = 0;
    pthread_mutex_unlock(&capture->mutex);
    return count;
}

int nn_pw_capture_failed(NN_Pipewire_Capture *capture) {
    return capture == NULL || capture->failed;
}

void nn_pw_capture_destroy(NN_Pipewire_Capture *capture) {
    if (capture == NULL) return;
    if (capture->loop) pw_thread_loop_stop(capture->loop);
    if (capture->stream) pw_stream_destroy(capture->stream);
    if (capture->loop) pw_thread_loop_destroy(capture->loop);
    pthread_mutex_destroy(&capture->mutex);
    free(capture->ring);
    free(capture);
}
