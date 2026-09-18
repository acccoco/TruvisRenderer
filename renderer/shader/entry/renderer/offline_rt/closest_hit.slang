#include "lib/renderer/offline_rt/payload.slangi"
#include "lib/renderer/realtime_rt/surface_hit.slangi"

[shader("closesthit")]
void main_closest_hit(inout RtTracePayload payload, in BuiltInTriangleIntersectionAttributes attr)
{
    RtSurfaceEvaluator::evaluate(payload, attr);
}
