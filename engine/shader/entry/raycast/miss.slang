#include "./payload.slangi"

[shader("miss")]
void main_miss(inout RayCastPayload payload)
{
    payload.hit = false;
}
