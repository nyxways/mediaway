/*
 * container.hpp — umbrella header for every mediaway::container::* class.
 *
 * All 8 mediaway-container formats: MP4/WebM share the typestated
 * Muxer/LiveMuxer/Demuxer classes (mp4_webm.hpp); Ogg/ADTS/FLV/MPEG-TS/MP3/
 * WAV each get dedicated classes reflecting their own C ABI shape — see each
 * header's top comment and the matching adr/container/000{4,5,6,7,8}-*.md.
 */

#ifndef MEDIAWAY_CONTAINER_HPP
#define MEDIAWAY_CONTAINER_HPP

#include <mediaway/container/adts.hpp>
#include <mediaway/container/detail.hpp>
#include <mediaway/container/flv.hpp>
#include <mediaway/container/mp3.hpp>
#include <mediaway/container/mp4_webm.hpp>
#include <mediaway/container/ogg.hpp>
#include <mediaway/container/ts.hpp>
#include <mediaway/container/wav.hpp>

#endif  // MEDIAWAY_CONTAINER_HPP
