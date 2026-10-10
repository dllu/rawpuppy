// Diagnostic bridge only; loaded by the owned native CUDA allocation probe.
#include <c10/cuda/CUDACachingAllocator.h>
#include <cuda_runtime_api.h>
#include <algorithm>
#include <cstring>
#include <exception>
#include <stdexcept>

extern "C" int rawpuppy_cuda_budget(unsigned long long bytes, double *previous,
                                    char *error, unsigned long long capacity) {
    try {
        *previous = c10::cuda::CUDACachingAllocator::getMemoryFraction(0);
        size_t free_bytes = 0, total_bytes = 0;
        if (cudaMemGetInfo(&free_bytes, &total_bytes) != cudaSuccess || !total_bytes)
            throw std::runtime_error("Cannot read CUDA memory capacity");
        c10::cuda::CUDACachingAllocator::emptyCache();
        c10::cuda::CUDACachingAllocator::setMemoryFraction(
            static_cast<double>(bytes) / static_cast<double>(total_bytes), 0);
        return 0;
    } catch (const std::exception &exception) {
        if (capacity) {
            const auto length = std::min<unsigned long long>(std::strlen(exception.what()), capacity - 1);
            std::memcpy(error, exception.what(), length);
            error[length] = '\0';
        }
        return 1;
    }
}

extern "C" int rawpuppy_cuda_restore(double fraction) {
    try {
        c10::cuda::CUDACachingAllocator::setMemoryFraction(fraction, 0);
        c10::cuda::CUDACachingAllocator::emptyCache();
        return 0;
    } catch (...) { return 1; }
}
