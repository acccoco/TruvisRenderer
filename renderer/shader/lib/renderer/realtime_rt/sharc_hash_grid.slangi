/// @file sharc_hash_grid.slangi
/// @brief SHARC 世界空间哈希网格（移植自 NVIDIA RTXGI SHARC `HashGridCommon.h`，v1.6）。
///
/// 仅做 Slang 适配：强制 64-bit atomics 分支（移除 lock buffer 与 GLSL 分支），其余哈希、
/// level、voxel、insert/find 逻辑与上游保持一致。buffer 以 `RWStructuredBuffer` 成员形式由
/// `HashMapData` 承载，调用方从 pass-local 绑定的全局 buffer 填入。坐标系与上游一致：
/// 世界空间位置经 `sceneScale` / `logarithmBase` 映射到对数分层 voxel。
///
/// 关键不变量：
/// - `HASH_GRID_INVALID_HASH_KEY = 0`，因此所有 buffer 初始必须清 0。
/// - hash key 抢占使用 64-bit `InterlockedCompareExchange`，需要 `shaderBufferInt64Atomics`。
#pragma once

// Constants
#define HASH_GRID_POSITION_BIT_NUM          17
#define HASH_GRID_POSITION_BIT_MASK         ((1u << HASH_GRID_POSITION_BIT_NUM) - 1)
#define HASH_GRID_LEVEL_BIT_NUM             10
#define HASH_GRID_LEVEL_BIT_MASK            ((1u << HASH_GRID_LEVEL_BIT_NUM) - 1)
#define HASH_GRID_NORMAL_BIT_NUM            3
#define HASH_GRID_NORMAL_BIT_MASK           ((1u << HASH_GRID_NORMAL_BIT_NUM) - 1)
#define HASH_GRID_HASH_MAP_BUCKET_SIZE      16
#define HASH_GRID_INVALID_HASH_KEY          0
#define HASH_GRID_INVALID_CACHE_INDEX       0xFFFFFFFF

// Tweakable parameters
#ifndef HASH_GRID_USE_NORMALS
#define HASH_GRID_USE_NORMALS               1       // 把法线纳入 hash key，区分同一 voxel 的正反面
#endif

#ifndef HASH_GRID_POSITION_OFFSET
#define HASH_GRID_POSITION_OFFSET           float3(0.0f, 0.0f, 0.0f)
#endif

#ifndef HASH_GRID_POSITION_BIAS
#define HASH_GRID_POSITION_BIAS             1e-4f   // 极端场景尺度下可能需要调整
#endif

#ifndef HASH_GRID_NORMAL_BIAS
#define HASH_GRID_NORMAL_BIAS               1e-3f
#endif

#define HashGridIndex uint
#define HashGridKey uint64_t

#ifndef HASH_GRID_LOOP_ATTR
#define HASH_GRID_LOOP_ATTR [loop]
#endif

struct HashGridParameters
{
    float3 cameraPosition;
    float logarithmBase;
    float sceneScale;
    float levelBias;
};

float HashGridLogBase(float x, float base)
{
    return log(x) / log(base);
}

// http://burtleburtle.net/bob/hash/integer.html
uint HashGridHashJenkins32(uint a)
{
    a = (a + 0x7ed55d16) + (a << 12);
    a = (a ^ 0xc761c23c) ^ (a >> 19);
    a = (a + 0x165667b1) + (a << 5);
    a = (a + 0xd3a2646c) ^ (a << 9);
    a = (a + 0xfd7046c5) + (a << 3);
    a = (a ^ 0xb55a4f09) ^ (a >> 16);

    return a;
}

uint HashGridHash32(HashGridKey hashKey)
{
    return HashGridHashJenkins32(uint((hashKey >> 0) & 0xFFFFFFFF)) ^ HashGridHashJenkins32(uint((hashKey >> 32) & 0xFFFFFFFF));
}

uint HashGridGetBaseSlot(const HashGridKey hashKey, uint capacity)
{
    uint hash = HashGridHash32(hashKey);
    uint slot = hash % capacity;

    return slot;
}

uint HashGridGetLevel(float3 samplePosition, HashGridParameters gridParameters)
{
    const float distance2 = dot(gridParameters.cameraPosition - samplePosition, gridParameters.cameraPosition - samplePosition);

    return uint(clamp(0.5f * HashGridLogBase(distance2, gridParameters.logarithmBase) + gridParameters.levelBias, 1.0f, float(HASH_GRID_LEVEL_BIT_MASK)));
}

float HashGridGetVoxelSize(uint gridLevel, HashGridParameters gridParameters)
{
    return pow(gridParameters.logarithmBase, gridLevel) / (gridParameters.sceneScale * pow(gridParameters.logarithmBase, gridParameters.levelBias));
}

// Based on logarithmic caching by Johannes Jendersie
int4 HashGridCalculatePositionLog(float3 samplePosition, HashGridParameters gridParameters)
{
    samplePosition += float3(HASH_GRID_POSITION_BIAS, HASH_GRID_POSITION_BIAS, HASH_GRID_POSITION_BIAS);

    uint  gridLevel     = HashGridGetLevel(samplePosition, gridParameters);
    float voxelSize     = HashGridGetVoxelSize(gridLevel, gridParameters);
    int3  gridPosition  = int3(floor(samplePosition / voxelSize));

    return int4(gridPosition.xyz, gridLevel);
}

HashGridKey HashGridComputeSpatialHash(float3 samplePosition, float3 sampleNormal, HashGridParameters gridParameters)
{
    uint4 gridPosition = uint4(HashGridCalculatePositionLog(samplePosition, gridParameters));

    HashGridKey hashKey = ((uint64_t(gridPosition.x) & HASH_GRID_POSITION_BIT_MASK) << (HASH_GRID_POSITION_BIT_NUM * 0)) |
                          ((uint64_t(gridPosition.y) & HASH_GRID_POSITION_BIT_MASK) << (HASH_GRID_POSITION_BIT_NUM * 1)) |
                          ((uint64_t(gridPosition.z) & HASH_GRID_POSITION_BIT_MASK) << (HASH_GRID_POSITION_BIT_NUM * 2)) |
                          ((uint64_t(gridPosition.w) & HASH_GRID_LEVEL_BIT_MASK) << (HASH_GRID_POSITION_BIT_NUM * 3));

#if HASH_GRID_USE_NORMALS
    uint normalBits =
        (sampleNormal.x + HASH_GRID_NORMAL_BIAS >= 0 ? 0 : 1) +
        (sampleNormal.y + HASH_GRID_NORMAL_BIAS >= 0 ? 0 : 2) +
        (sampleNormal.z + HASH_GRID_NORMAL_BIAS >= 0 ? 0 : 4);

    hashKey |= (uint64_t(normalBits) << (HASH_GRID_POSITION_BIT_NUM * 3 + HASH_GRID_LEVEL_BIT_NUM));
#endif // HASH_GRID_USE_NORMALS

    return hashKey;
}

float3 HashGridGetPositionFromKey(const HashGridKey hashKey, HashGridParameters gridParameters)
{
    const int signBit      = 1 << (HASH_GRID_POSITION_BIT_NUM - 1);
    const int signMask     = ~((1 << HASH_GRID_POSITION_BIT_NUM) - 1);

    int3 gridPosition;
    gridPosition.x = int((hashKey >> (HASH_GRID_POSITION_BIT_NUM * 0)) & HASH_GRID_POSITION_BIT_MASK);
    gridPosition.y = int((hashKey >> (HASH_GRID_POSITION_BIT_NUM * 1)) & HASH_GRID_POSITION_BIT_MASK);
    gridPosition.z = int((hashKey >> (HASH_GRID_POSITION_BIT_NUM * 2)) & HASH_GRID_POSITION_BIT_MASK);

    // Fix negative coordinates
    gridPosition.x = (gridPosition.x & signBit) != 0 ? gridPosition.x | signMask : gridPosition.x;
    gridPosition.y = (gridPosition.y & signBit) != 0 ? gridPosition.y | signMask : gridPosition.y;
    gridPosition.z = (gridPosition.z & signBit) != 0 ? gridPosition.z | signMask : gridPosition.z;

    uint   gridLevel        = uint((hashKey >> HASH_GRID_POSITION_BIT_NUM * 3) & HASH_GRID_LEVEL_BIT_MASK);
    float  voxelSize        = HashGridGetVoxelSize(gridLevel, gridParameters);
    float3 samplePosition   = (gridPosition + 0.5f) * voxelSize;

    return samplePosition;
}

struct HashMapData
{
    uint capacity;

    RWStructuredBuffer<uint64_t> hashEntriesBuffer;
};

void HashMapAtomicCompareExchange(in HashMapData hashMapData, in uint dstOffset, in uint64_t compareValue, in uint64_t value, out uint64_t originalValue)
{
    // 仅保留 64-bit atomics 分支：truvis 设备已开启 shaderBufferInt64Atomics，
    // 不使用上游 lock buffer 回退（其在 fuse 无限时存在 device hang 风险）。
    InterlockedCompareExchange(hashMapData.hashEntriesBuffer[dstOffset], compareValue, value, originalValue);
}

bool HashMapInsert(in HashMapData hashMapData, const HashGridKey hashKey, out HashGridIndex cacheIndex)
{
    const uint baseSlot = HashGridGetBaseSlot(hashKey, hashMapData.capacity);
    HASH_GRID_LOOP_ATTR
    for (uint bucketOffset = 0; bucketOffset < HASH_GRID_HASH_MAP_BUCKET_SIZE; ++bucketOffset)
    {
        HashGridKey prevHashGridKey;
        uint slotIndex = (baseSlot + bucketOffset) % hashMapData.capacity;
        HashMapAtomicCompareExchange(hashMapData, slotIndex, HASH_GRID_INVALID_HASH_KEY, hashKey, prevHashGridKey);

        if (prevHashGridKey == HASH_GRID_INVALID_HASH_KEY || prevHashGridKey == hashKey)
        {
            cacheIndex = slotIndex;
            return true;
        }
    }

    cacheIndex = 0;

    return false;
}

bool HashMapFind(in HashMapData hashMapData, const HashGridKey hashKey, inout HashGridIndex cacheIndex, out uint bucketOffset)
{
    const uint baseSlot = HashGridGetBaseSlot(hashKey, hashMapData.capacity);
    HASH_GRID_LOOP_ATTR
    for (bucketOffset = 0; bucketOffset < HASH_GRID_HASH_MAP_BUCKET_SIZE; ++bucketOffset)
    {
        uint slotIndex = (baseSlot + bucketOffset) % hashMapData.capacity;
        HashGridKey storedHashKey = hashMapData.hashEntriesBuffer[slotIndex];

        if (storedHashKey == hashKey)
        {
            cacheIndex = slotIndex;
            return true;
        }
    }

    return false;
}

HashGridIndex HashMapInsertEntry(in HashMapData hashMapData, float3 samplePosition, float3 sampleNormal, HashGridParameters gridParameters)
{
    HashGridIndex cacheIndex    = HASH_GRID_INVALID_CACHE_INDEX;
    const HashGridKey hashKey   = HashGridComputeSpatialHash(samplePosition, sampleNormal, gridParameters);
    bool successful             = HashMapInsert(hashMapData, hashKey, cacheIndex);

    return cacheIndex;
}

HashGridIndex HashMapFindEntry(in HashMapData hashMapData, float3 samplePosition, float3 sampleNormal, HashGridParameters gridParameters)
{
    HashGridIndex cacheIndex    = HASH_GRID_INVALID_CACHE_INDEX;
    const HashGridKey hashKey   = HashGridComputeSpatialHash(samplePosition, sampleNormal, gridParameters);
    uint hashCollisionsNum;
    bool successful             = HashMapFind(hashMapData, hashKey, cacheIndex, hashCollisionsNum);

    return cacheIndex;
}

// Debug functions
float3 HashGridGetColorFromHash32(uint hash)
{
    float3 color;
    color.x = ((hash >>  0) & 0x3ff) / 1023.0f;
    color.y = ((hash >> 11) & 0x7ff) / 2047.0f;
    color.z = ((hash >> 22) & 0x7ff) / 2047.0f;

    return color;
}

// Debug visualization
float3 HashGridDebugColoredHash(float3 samplePosition, float3 sampleNormal, HashGridParameters gridParameters)
{
    HashGridKey hashKey     = HashGridComputeSpatialHash(samplePosition, sampleNormal, gridParameters);
    uint gridLevel          = HashGridGetLevel(samplePosition, gridParameters);
    float3 color            = HashGridGetColorFromHash32(HashGridHash32(hashKey)) * HashGridGetColorFromHash32(HashGridHashJenkins32(gridLevel)).xyz;

    return color;
}

float3 HashGridDebugOccupancy(uint2 pixelPosition, uint2 screenSize, HashMapData hashMapData)
{
    const uint elementSize = 7;
    const uint borderSize = 1;
    const uint blockSize = elementSize + borderSize;

    uint rowNum = screenSize.y / blockSize;
    uint rowIndex = pixelPosition.y / blockSize;
    uint columnIndex = pixelPosition.x / blockSize;
    uint elementIndex = (columnIndex / HASH_GRID_HASH_MAP_BUCKET_SIZE) * (rowNum * HASH_GRID_HASH_MAP_BUCKET_SIZE) + rowIndex * HASH_GRID_HASH_MAP_BUCKET_SIZE + (columnIndex % HASH_GRID_HASH_MAP_BUCKET_SIZE);

    if (elementIndex < hashMapData.capacity && ((pixelPosition.x % blockSize) < elementSize && (pixelPosition.y % blockSize) < elementSize))
    {
        HashGridKey storedHashGridKey = hashMapData.hashEntriesBuffer[elementIndex];
        if (storedHashGridKey != HASH_GRID_INVALID_HASH_KEY)
            return float3(0.0f, 1.0f, 0.0f);
    }

    return float3(0.0f, 0.0f, 0.0f);
}
