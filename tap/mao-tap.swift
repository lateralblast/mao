// mao-tap: system audio and output helper. Modes:
//
//   mao-tap [--pass]        copy system audio to stdout as 48 kHz, 16-bit, stereo PCM (for AirPlay).
//                           The Mac's output is muted, unless --pass leaves it playing.
//   mao-tap devices         print this Mac's output devices as JSON
//   mao-tap output          print the system output device's UID
//   mao-tap set-output UID  make the output device UID the system output
//   mao-tap record SECONDS PATH  record the microphone to a WAV file (for sync calibration)
//
// Capture runs until stdin closes (mao has exited) or it gets SIGTERM, and then removes what it created.

import AVFoundation
import CoreAudio
import Foundation

signal(SIGPIPE, SIG_IGN)

let stdoutFD: Int32 = 1
let logPrefix = "mao-tap"

func fail(_ message: String, _ code: Int32 = 1) -> Never {
    fputs("\(logPrefix): \(message)\n", stderr)
    exit(code)
}

// MARK: - Core Audio helpers

func address(_ selector: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
}

func stringProperty(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    // The HAL returns a retained CFString reference, so it is read as Unmanaged and released here.
    var addr = address(selector)
    var value: Unmanaged<CFString>? = nil
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(object, &addr, 0, nil, &size, &value) == 0, let reference = value else { return nil }
    return reference.takeRetainedValue() as String
}

func uintProperty(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> UInt32? {
    var addr = address(selector)
    var value: UInt32 = 0
    var size = UInt32(MemoryLayout<UInt32>.size)
    guard AudioObjectGetPropertyData(object, &addr, 0, nil, &size, &value) == 0 else { return nil }
    return value
}

func hasOutputStreams(_ object: AudioObjectID) -> Bool {
    var addr = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput)
    var size: UInt32 = 0
    return AudioObjectGetPropertyDataSize(object, &addr, 0, nil, &size) == 0 && size > 0
}

func allDeviceIDs() -> [AudioObjectID] {
    var addr = address(kAudioHardwarePropertyDevices)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size) == 0 else { return [] }
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &ids) == 0 else { return [] }
    return ids
}

func deviceID(forUID uid: String) -> AudioObjectID? {
    allDeviceIDs().first { stringProperty($0, kAudioDevicePropertyDeviceUID) == uid }
}

/// The system output device's UID.
func systemOutputUID() -> String? {
    var addr = address(kAudioHardwarePropertyDefaultSystemOutputDevice)
    var dev = AudioObjectID(0)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &dev) == 0 else { return nil }
    return stringProperty(dev, kAudioDevicePropertyDeviceUID)
}

/// Makes `uid` the default output and the system-sound output.
func setSystemOutput(uid: String) -> Bool {
    guard var device = deviceID(forUID: uid) else { return false }
    let size = UInt32(MemoryLayout<AudioObjectID>.size)
    var ok = true
    for selector in [kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDefaultSystemOutputDevice] {
        var addr = address(selector)
        ok = ok && AudioObjectSetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, size, &device) == 0
    }
    return ok
}

/// A tap that captures the whole system mix. It mutes the Mac's output unless `mute` is false.
func makeTap(mute: Bool) -> (AudioObjectID, String) {
    let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
    desc.uuid = UUID()
    desc.muteBehavior = mute ? .muted : .unmuted
    desc.isPrivate = true
    var tapID = AudioObjectID(kAudioObjectUnknown)
    guard AudioHardwareCreateProcessTap(desc, &tapID) == 0 else {
        fail("could not create the system audio tap (is audio capture allowed for mao?)")
    }
    return (tapID, desc.uuid.uuidString)
}

func makeAggregate(name: String, mainUID: String, subUIDs: [String], tapUUID: String) -> AudioObjectID {
    let description: [String: Any] = [
        kAudioAggregateDeviceNameKey: name,
        kAudioAggregateDeviceUIDKey: UUID().uuidString,
        kAudioAggregateDeviceMainSubDeviceKey: mainUID,
        kAudioAggregateDeviceIsPrivateKey: true,
        kAudioAggregateDeviceTapAutoStartKey: true,
        kAudioAggregateDeviceSubDeviceListKey: subUIDs.map { [kAudioSubDeviceUIDKey: $0] },
        kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: tapUUID, kAudioSubTapDriftCompensationKey: true]],
    ]
    var aggregateID = AudioObjectID(kAudioObjectUnknown)
    guard AudioHardwareCreateAggregateDevice(description as CFDictionary, &aggregateID) == 0 else {
        fail("could not create the capture device")
    }
    return aggregateID
}

/// Blocks until stdin closes, then runs `cleanup` and exits. A SIGTERM does the same.
func exitWhenParentGoes(_ cleanup: @escaping () -> Void) {
    DispatchQueue.global().async {
        var buffer = [UInt8](repeating: 0, count: 64)
        while read(0, &buffer, buffer.count) > 0 {}
        cleanup()
        exit(0)
    }
    signal(SIGTERM, SIG_IGN)
    let terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
    terminate.setEventHandler {
        cleanup()
        exit(0)
    }
    terminate.resume()
}

// MARK: - Mode: capture to stdout (AirPlay)

/// The index of the first buffer of the tap's stereo input. An aggregate's input can also carry
/// its main device's inputs, listed first, so the tap is the last two channels.
func tapInputStart(_ list: UnsafeMutableAudioBufferListPointer) -> Int {
    var channels = 0
    var i = list.count
    while i > 0 && channels < 2 {
        i -= 1
        channels += max(1, Int(list[i].mNumberChannels))
    }
    return i
}

func runCapture(mute: Bool) -> Never {
    let (tapID, tapUUID) = makeTap(mute: mute)
    guard let outUID = systemOutputUID() else { fail("could not read the system output device") }
    let aggregateID = makeAggregate(name: "mao-tap", mainUID: outUID, subUIDs: [outUID], tapUUID: tapUUID)

    var procID: AudioDeviceIOProcID?
    func cleanup() {
        if let procID = procID {
            AudioDeviceStop(aggregateID, procID)
            AudioDeviceDestroyIOProcID(aggregateID, procID)
        }
        AudioHardwareDestroyAggregateDevice(aggregateID)
        AudioHardwareDestroyProcessTap(tapID)
    }

    let block: AudioDeviceIOBlock = { _, inData, _, outData, _ in
        // This aggregate's output goes to the system output device. Write silence there, so the
        // buffers' old contents are not played.
        for buffer in UnsafeMutableAudioBufferListPointer(outData) {
            if let data = buffer.mData {
                memset(data, 0, Int(buffer.mDataByteSize))
            }
        }
        let input = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inData))
        let start = tapInputStart(input)
        var out: [Int16] = []
        if input.count - start == 1, let data = input[start].mData {
            // Interleaved float32, stereo.
            let channels = max(1, Int(input[start].mNumberChannels))
            let frames = Int(input[start].mDataByteSize) / (MemoryLayout<Float32>.size * channels)
            let samples = data.bindMemory(to: Float32.self, capacity: frames * channels)
            out.reserveCapacity(frames * 2)
            for i in 0..<frames {
                out.append(Int16(max(-1, min(1, samples[i * channels])) * 32767))
                out.append(Int16(max(-1, min(1, samples[i * channels + (channels > 1 ? 1 : 0)])) * 32767))
            }
        } else if input.count - start >= 2, let left = input[start].mData, let right = input[start + 1].mData {
            // One buffer per channel.
            let frames = Int(input[start].mDataByteSize) / MemoryLayout<Float32>.size
            let l = left.bindMemory(to: Float32.self, capacity: frames)
            let r = right.bindMemory(to: Float32.self, capacity: frames)
            out.reserveCapacity(frames * 2)
            for i in 0..<frames {
                out.append(Int16(max(-1, min(1, l[i])) * 32767))
                out.append(Int16(max(-1, min(1, r[i])) * 32767))
            }
        }
        if !out.isEmpty && !writeAll(out) {
            cleanup()
            exit(0)
        }
    }
    guard AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil, block) == 0 else {
        cleanup()
        fail("could not create the audio callback")
    }
    guard AudioDeviceStart(aggregateID, procID) == 0 else {
        cleanup()
        fail("could not start capture")
    }
    exitWhenParentGoes(cleanup)
    dispatchMain()
}

/// Writes all bytes to stdout. Returns false when the reader has gone away.
func writeAll(_ samples: [Int16]) -> Bool {
    samples.withUnsafeBytes { raw in
        var offset = 0
        while offset < raw.count {
            let n = write(stdoutFD, raw.baseAddress! + offset, raw.count - offset)
            if n <= 0 { return false }
            offset += n
        }
        return true
    }
}

// MARK: - Modes: devices, output

func runDevices() -> Never {
    var found: [(name: String, uid: String)] = []
    for id in allDeviceIDs() {
        guard hasOutputStreams(id) else { continue }
        // AirPlay outputs are handled by the streamer, and the helper's own devices are not listed.
        if uintProperty(id, kAudioDevicePropertyTransportType) == kAudioDeviceTransportTypeAirPlay { continue }
        guard let uid = stringProperty(id, kAudioDevicePropertyDeviceUID) else { continue }
        let name = stringProperty(id, kAudioObjectPropertyName) ?? uid
        if name.hasPrefix("mao-") { continue }
        found.append((name, uid))
    }
    found.sort { $0.name.lowercased() < $1.name.lowercased() }
    let list: [[String: String]] = found.map { ["identifier": "local:" + $0.uid, "name": $0.name, "address": ""] }
    let data = (try? JSONSerialization.data(withJSONObject: list)) ?? Data("[]".utf8)
    FileHandle.standardOutput.write(data)
    exit(0)
}

func runOutput() -> Never {
    guard let uid = systemOutputUID() else { fail("could not read the system output device") }
    print(uid)
    exit(0)
}

func runSetOutput(uid: String) -> Never {
    guard setSystemOutput(uid: uid) else { fail("could not set the system output to \(uid)") }
    exit(0)
}

// MARK: - Volume and mute of an output device

/// The device's output volume scalar on `element`, or nil if it has none there.
func volumeScalar(_ device: AudioObjectID, element: UInt32) -> Float32? {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyVolumeScalar,
                                          mScope: kAudioDevicePropertyScopeOutput, mElement: element)
    guard AudioObjectHasProperty(device, &addr) else { return nil }
    var value: Float32 = 0
    var size = UInt32(MemoryLayout<Float32>.size)
    guard AudioObjectGetPropertyData(device, &addr, 0, nil, &size, &value) == 0 else { return nil }
    return value
}

func setVolumeScalar(_ device: AudioObjectID, element: UInt32, _ value: Float32) -> Bool {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyVolumeScalar,
                                          mScope: kAudioDevicePropertyScopeOutput, mElement: element)
    guard AudioObjectHasProperty(device, &addr) else { return false }
    var v = value
    return AudioObjectSetPropertyData(device, &addr, 0, nil, UInt32(MemoryLayout<Float32>.size), &v) == 0
}

/// The output volume as 0 to 100: the master element if the device has one, else the left channel.
func outputLevel(_ device: AudioObjectID) -> Double? {
    let value = volumeScalar(device, element: 0) ?? volumeScalar(device, element: 1)
    return value.map { Double($0) * 100 }
}

/// Sets the output volume (0 to 100) on the master element, or on both channels if there is none.
func setOutputLevel(_ device: AudioObjectID, _ level: Double) -> Bool {
    let value = Float32(max(0, min(100, level)) / 100)
    if setVolumeScalar(device, element: 0, value) { return true }
    let left = setVolumeScalar(device, element: 1, value)
    let right = setVolumeScalar(device, element: 2, value)
    return left || right
}

func outputMuted(_ device: AudioObjectID) -> Bool? {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyMute,
                                          mScope: kAudioDevicePropertyScopeOutput, mElement: 0)
    guard AudioObjectHasProperty(device, &addr) else { return nil }
    var value: UInt32 = 0
    var size = UInt32(MemoryLayout<UInt32>.size)
    guard AudioObjectGetPropertyData(device, &addr, 0, nil, &size, &value) == 0 else { return nil }
    return value != 0
}

func setOutputMuted(_ device: AudioObjectID, _ muted: Bool) -> Bool {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyMute,
                                          mScope: kAudioDevicePropertyScopeOutput, mElement: 0)
    guard AudioObjectHasProperty(device, &addr) else { return false }
    var value: UInt32 = muted ? 1 : 0
    return AudioObjectSetPropertyData(device, &addr, 0, nil, UInt32(MemoryLayout<UInt32>.size), &value) == 0
}

/// Prints "LEVEL MUTED" for the output device `uid`, e.g. "49.0 0".
func runVolume(uid: String) -> Never {
    guard let device = deviceID(forUID: uid), let level = outputLevel(device) else {
        fail("this output has no volume control")
    }
    let muted = outputMuted(device) ?? false
    print(String(format: "%.1f %d", level, muted ? 1 : 0))
    exit(0)
}

func runSetVolume(uid: String, level: Double) -> Never {
    guard let device = deviceID(forUID: uid), setOutputLevel(device, level) else {
        fail("could not set the volume of \(uid)")
    }
    exit(0)
}

func runSetMute(uid: String, muted: Bool) -> Never {
    guard let device = deviceID(forUID: uid), setOutputMuted(device, muted) else {
        fail("this output cannot be muted")
    }
    exit(0)
}

// MARK: - Mode: record the microphone

/// Records the default microphone for `seconds` into a 16-bit mono WAV at `path`, and exits.
/// The file is closed before exit, so its header is complete.
func runRecord(seconds: Double, path: String) -> Never {
    func record() throws {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: format.sampleRate,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsNonInterleaved: false,
        ]
        var file: AVAudioFile? = try AVAudioFile(forWriting: URL(fileURLWithPath: path), settings: settings,
                                                 commonFormat: .pcmFormatFloat32, interleaved: false)
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, _ in
            do { try file?.write(from: buffer) } catch { fputs("mao-tap: write failed\n", stderr) }
        }
        try engine.start()
        Thread.sleep(forTimeInterval: seconds)
        input.removeTap(onBus: 0)
        engine.stop()
        file = nil  // closing the file writes its header
    }
    do {
        try record()
    } catch {
        fail("could not record the microphone: \(error.localizedDescription)")
    }
    exit(0)
}

// MARK: - Entry point

let arguments = Array(CommandLine.arguments.dropFirst())
switch arguments.first {
case "devices": runDevices()
case "output": runOutput()
case "volume":
    guard arguments.count >= 2 else { fail("volume needs a device UID") }
    runVolume(uid: arguments[1])
case "set-volume":
    guard arguments.count >= 3, let level = Double(arguments[2]) else { fail("set-volume needs a UID and a level") }
    runSetVolume(uid: arguments[1], level: level)
case "set-mute":
    guard arguments.count >= 3 else { fail("set-mute needs a UID and 0 or 1") }
    runSetMute(uid: arguments[1], muted: arguments[2] == "1")
case "record":
    guard arguments.count >= 3, let seconds = Double(arguments[1]) else { fail("record needs SECONDS and PATH") }
    runRecord(seconds: seconds, path: arguments[2])
case "set-output":
    guard arguments.count >= 2 else { fail("set-output needs a device UID") }
    runSetOutput(uid: arguments[1])
default:
    runCapture(mute: !arguments.contains("--pass"))
}
