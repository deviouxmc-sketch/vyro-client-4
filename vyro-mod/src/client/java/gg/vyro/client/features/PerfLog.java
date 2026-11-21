package gg.vyro.client.features;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import gg.vyro.client.Metrics;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * Records how each play session actually performed (one FPS sample per second) into
 * {@code config/vyro/perf.json}. The launcher's Performance page reads it so players can compare
 * presets and mods using their own numbers.
 */
public final class PerfLog {
    private PerfLog() {}

    private static final int MAX_SESSIONS = 20;
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();

    static final class Session {
        long startedAt;
        long durationSec;
        double avgFps;
        double low1Fps;
        long maxHeapMb;
        int mods;
        String mcVersion = "";
    }

    static final class PerfFile {
        List<Session> sessions = new ArrayList<>();
    }

    private static final List<Integer> SAMPLES = new ArrayList<>();
    private static long startedAt;
    private static boolean started;
    private static int ticks;

    public static void init() {
        Runtime.getRuntime().addShutdownHook(new Thread(PerfLog::flush, "vyro-perf-flush"));
    }

    /** Called every client tick. */
    public static void tick(Minecraft mc) {
        if (mc.player == null) return;
        if (!started) {
            started = true;
            startedAt = System.currentTimeMillis();
        }
        if (++ticks % 20 != 0) return;
        int fps = Metrics.fps();
        if (fps > 0) {
            SAMPLES.add(fps);
            if (SAMPLES.size() % 60 == 0) flush();
        }
    }

    private static Path file() {
        return FabricLoader.getInstance().getConfigDir().resolve("vyro").resolve("perf.json");
    }

    private static synchronized void flush() {
        int n = SAMPLES.size();
        if (n < 10) return; // too short to mean anything
        try {
            List<Integer> sorted = new ArrayList<>(SAMPLES);
            Collections.sort(sorted);
            double sum = 0;
            for (int v : sorted) sum += v;
            int k = Math.max(1, n / 100);
            double low = 0;
            for (int i = 0; i < k; i++) low += sorted.get(i);

            Session s = new Session();
            s.startedAt = startedAt;
            s.durationSec = n;
            s.avgFps = Math.round(sum / n * 10.0) / 10.0;
            s.low1Fps = Math.round(low / k * 10.0) / 10.0;
            s.maxHeapMb = Runtime.getRuntime().maxMemory() / (1024 * 1024);
            s.mods = FabricLoader.getInstance().getAllMods().size();
            s.mcVersion = FabricLoader.getInstance().getModContainer("minecraft")
                    .map(c -> c.getMetadata().getVersion().getFriendlyString()).orElse("");

            Path f = file();
            PerfFile pf = new PerfFile();
            if (Files.isRegularFile(f)) {
                try {
                    PerfFile old = GSON.fromJson(Files.readString(f, StandardCharsets.UTF_8), PerfFile.class);
                    if (old != null && old.sessions != null) pf = old;
                } catch (Exception ignored) {
                    // unreadable file: start a fresh one
                }
            }
            pf.sessions.removeIf(x -> x.startedAt == s.startedAt);
            pf.sessions.add(s);
            while (pf.sessions.size() > MAX_SESSIONS) pf.sessions.remove(0);

            Files.createDirectories(f.getParent());
            Path tmp = f.resolveSibling("perf.json.tmp");
            Files.writeString(tmp, GSON.toJson(pf), StandardCharsets.UTF_8);
            Files.move(tmp, f, StandardCopyOption.REPLACE_EXISTING);
        } catch (Exception e) {
            System.err.println("[VYRO] could not write performance log: " + e);
        }
    }
}
