// mao-tap: copies system audio to stdout as 48 kHz, 16-bit, stereo PCM.
//
// The tap leaves the normal output untouched, so the Mac still plays through
// its own speakers. It runs until stdin closes (mao has exited) or it gets SIGTERM.

import CoreAudio
import Foundation

signal(SIGPIPE, SIG_IGN)

func defaultSystemOutputUID() -> String? {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultSystemOutputDevice,
                                          mScope: kAudioObjectPropertyScopeGlobal,
                                          mElement: kAudioObjectPropertyElementMain)
    var dev = AudioObjectID(0)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &dev) == 0 else {
        return nil
    }
    var uidAddr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyDeviceUID,
                                             mScope: kAudioObjectPropertyScopeGlobal,
                                             mElement: kAudioObjectPropertyElementMain)
    var uid: CFString = "" as CFString
    var uidSize = UInt32(MemoryLayout<CFString>.size)
    guard AudioObjectGetPropertyData(dev, &uidAddr, 0, nil, &uidSize, &uid) == 0 else { return nil }
    return uid as String
}

/// Writes all bytes to stdout. Returns false when the reader has gone away.
func writeAll(_ bytes: [Int16]) -> Bool {
    bytes.withUnsafeBytes { raw in
        var offset = 0
        while offset < raw.count {
            let n = write(1, raw.baseAddress! + offset, raw.count - offset)
            if n <= 0 { return false }
            offset += n
        }
        return true
    }
}

let tapDesc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
tapDesc.uuid = UUID()
tapDesc.muteBehavior = .unmuted
tapDesc.isPrivate = true

var tapID = AudioObjectID(kAudioObjectUnknown)
guard AudioHardwareCreateProcessTap(tapDesc, &tapID) == 0 else {
    fputs("mao-tap: could not create the system audio tap (is audio capture allowed for mao?)\n", stderr)
    exit(1)
}

guard let outUID = defaultSystemOutputUID() else {
    fputs("mao-tap: could not read the system output device\n", stderr)
    exit(1)
}

let aggregateDescription: [String: Any] = [
    kAudioAggregateDeviceNameKey: "mao-tap",
    kAudioAggregateDeviceUIDKey: UUID().uuidString,
    kAudioAggregateDeviceMainSubDeviceKey: outUID,
    kAudioAggregateDeviceIsPrivateKey: true,
    kAudioAggregateDeviceTapAutoStartKey: true,
    kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outUID]],
    kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: tapDesc.uuid.uuidString,
                                       kAudioSubTapDriftCompensationKey: true]],
]
var aggregateID = AudioObjectID(kAudioObjectUnknown)
guard AudioHardwareCreateAggregateDevice(aggregateDescription as CFDictionary, &aggregateID) == 0 else {
    fputs("mao-tap: could not create the capture device\n", stderr)
    AudioHardwareDestroyProcessTap(tapID)
    exit(1)
}

var procID: AudioDeviceIOProcID?

func cleanupAndExit(_ code: Int32) -> Never {
    if let procID = procID {
        AudioDeviceStop(aggregateID, procID)
        AudioDeviceDestroyIOProcID(aggregateID, procID)
    }
    AudioHardwareDestroyAggregateDevice(aggregateID)
    AudioHardwareDestroyProcessTap(tapID)
    exit(code)
}

let ioBlock: AudioDeviceIOBlock = { _, inData, _, _, _ in
    let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inData))
    var out: [Int16] = []
    if buffers.count == 1, let data = buffers[0].mData {
        // Interleaved float32, stereo.
        let count = Int(buffers[0].mDataByteSize) / MemoryLayout<Float32>.size
        let samples = data.bindMemory(to: Float32.self, capacity: count)
        out.reserveCapacity(count)
        for i in 0..<count {
            out.append(Int16(max(-1, min(1, samples[i])) * 32767))
        }
    } else if buffers.count >= 2, let left = buffers[0].mData, let right = buffers[1].mData {
        // Non-interleaved float32: one buffer per channel.
        let frames = Int(buffers[0].mDataByteSize) / MemoryLayout<Float32>.size
        let l = left.bindMemory(to: Float32.self, capacity: frames)
        let r = right.bindMemory(to: Float32.self, capacity: frames)
        out.reserveCapacity(frames * 2)
        for i in 0..<frames {
            out.append(Int16(max(-1, min(1, l[i])) * 32767))
            out.append(Int16(max(-1, min(1, r[i])) * 32767))
        }
    }
    if !out.isEmpty && !writeAll(out) {
        cleanupAndExit(0)
    }
}

guard AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil, ioBlock) == 0 else {
    fputs("mao-tap: could not create the audio callback\n", stderr)
    cleanupAndExit(1)
}
guard AudioDeviceStart(aggregateID, procID) == 0 else {
    fputs("mao-tap: could not start capture\n", stderr)
    cleanupAndExit(1)
}

// Exit when mao closes our stdin.
DispatchQueue.global().async {
    var buffer = [UInt8](repeating: 0, count: 64)
    while read(0, &buffer, buffer.count) > 0 {}
    cleanupAndExit(0)
}

// Exit cleanly on SIGTERM too.
signal(SIGTERM, SIG_IGN)
let terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
terminate.setEventHandler { cleanupAndExit(0) }
terminate.resume()

dispatchMain()
