package ameise.observer;

import ameise.client.LogicalUnit;
import ameise.shared.communication.*;
import ameise.shared.communication.content.*;
import java.net.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.ArrayBlockingQueue;

/** Observation only. Every public hook contains failures, and performs no network I/O. */
public final class ObservationHook {
    private static final ArrayBlockingQueue<String> QUEUE = new ArrayBlockingQueue<String>(256);
    private static final boolean ENABLED = Boolean.parseBoolean(System.getProperty("ameise.observer.enabled", "false"));
    private static String stream, request, command, name, date;
    private static int game, tournament, model;
    private static Integer turn;
    private static long sequence;
    private static boolean firstCommand, sendingCommand, workerStarted;
    private ObservationHook() {}

    public static synchronized void before(Object message) {
        if (!ENABLED) return;
        try {
            sendingCommand = false;
            if (!(message instanceof MessageObject)) return;
            MessageObject m = (MessageObject) message;
            if (m.getType() != 2 || !(m.getContent() instanceof RequestManagerObject)) return;
            Object content = ((RequestManagerObject) m.getContent()).getContent();
            if (!(content instanceof CommandObject) || stream == null) return;
            CommandObject c = (CommandObject) content;
            if (c.getGameId() != game || c.getModelInstanceId() != model) { warn("Command identity does not match selected game; observation skipped"); return; }
            turn = nonnegative(c.getSpielstandId());
            if (!firstCommand && (c.getRestart() || c.getRollback())) { date = null; startStream(); }
            firstCommand = false;
            request = UUID.randomUUID().toString();
            command = c.getCommandString();
            sendingCommand = true;
        } catch (Throwable e) { warn("Cannot prepare command observation"); }
    }
    public static synchronized void sent() {
        if (!ENABLED) return;
        try { if (sendingCommand && request != null) emit("command.sent", payload()); }
        catch (Throwable e) { warn("Cannot capture sent observation"); }
        finally { sendingCommand = false; }
    }
    public static synchronized void failed() {
        if (!ENABLED) return;
        try { if (request != null) { Map<String,Object> p = payload(); p.put("message", "The desktop client could not establish the simulator outcome. Inspect the original client; do not replay the command."); emit("transport.uncertain", p); } }
        catch (Throwable e) { warn("Cannot capture uncertain transport"); }
    }
    /** Called after original processObject succeeds, before GUI delivery can send the next macro step. */
    public static synchronized void processed(boolean success, LogicalUnit logical, MessageObject message) {
        if (!ENABLED || !success || message == null) return;
        try {
            if (message.getType() == 1 && message.getContent() instanceof DBResultObject) {
                DBResultObject db = (DBResultObject) message.getContent();
                if (db.getMethodID() != 6 || !db.getErrorFlag()) return;
                Vector content = db.getContentVector();
                GameData g = (GameData) content.get(0);
                TurnData t = (TurnData) content.get(1);
                ModelInstanceData mi = (ModelInstanceData) ((Vector) content.get(3)).get(0);
                int tid = tournamentId(logical, g.getGameId());
                // A round ID is not a tournament ID. Never fabricate an identity when metadata is missing.
                stream = null; request = null;
                if (tid <= 0 || g.getGameId() <= 0 || mi.getModelInstanceId() <= 0) { warn("Selection metadata incomplete; observation stream unavailable"); return; }
                game = g.getGameId(); tournament = tid; model = mi.getModelInstanceId();
                name = mi.getName() == null || mi.getName().isEmpty() ? "Game " + game : mi.getName();
                date = emptyToNull(t.getDate()); turn = nonnegative(t.getTurnId());
                startStream();
            } else if (message.getType() == 3 && message.getContent() instanceof ResultObject && request != null) {
                ResultObject result = (ResultObject) message.getContent();
                if (result.getResultObjectId() == 2) {
                    CommandResultObject c = (CommandResultObject) result.getContentObject();
                    turn = nonnegative(c.getSpielstandId()); date = emptyToNull(c.getKernelDatString());
                    Map<String,Object> p = payload();
                    p.put("feedback", text(c.getFeedback())); p.put("kernel_output_raw", text(c.getKernelResultString())); p.put("simulation_date_raw", date);
                    emit("command.result", p); request = null;
                } else if (result.getResultObjectId() == 1) {
                    CallbackResultObject c = (CallbackResultObject) result.getContentObject();
                    turn = nonnegative(c.getSpielstandId());
                    Map<String,Object> p = payload();
                    p.put("question", text(c.getQuestionString()));
                    // Existing alternatives parser assumes single-digit IDs. Until captured fixtures
                    // establish the full grammar, preserve raw data and leave choices unavailable.
                    p.put("choices", Collections.emptyList()); p.put("alternatives_raw", text(c.getAlternativesString()));
                    List<Long> answers = new ArrayList<Long>();
                    if (c.getAnswerVector() != null) for (Object answer : c.getAnswerVector()) {
                        if (!(answer instanceof Number)) throw new IllegalArgumentException("Non-numeric answer ID");
                        answers.add(((Number) answer).longValue());
                    }
                    p.put("previous_answer_ids", answers); emit("callback.requested", p);
                } else if (result.getResultObjectId() == 4) { failed(); }
                // Wait hints (3) intentionally do not resolve the request.
            } else if (message.getType() == 6) { failed(); }
        } catch (Throwable e) { warn("Cannot capture received observation"); }
    }
    private static int tournamentId(LogicalUnit logical, int gid) {
        if (logical.getGames() == null) return -1;
        for (Object row : logical.getGames().unstructuedData) {
            if (!(row instanceof Vector)) continue;
            GameData g = null; TournamentData t = null;
            for (Object value : (Vector) row) { if (value instanceof GameData) g = (GameData)value; if (value instanceof TournamentData) t = (TournamentData)value; }
            if (g != null && t != null && g.getGameId() == gid) return t.getTournamentId();
        }
        return -1;
    }
    private static void startStream() {
        stream = UUID.randomUUID().toString(); sequence = 0; request = null; firstCommand = true;
        Map<String,Object> p = new LinkedHashMap<String,Object>(); p.put("name",name); p.put("simulation_date_raw",date); emit("game.selected",p);
    }
    private static Map<String,Object> payload() { Map<String,Object> p = new LinkedHashMap<String,Object>(); p.put("request_id",request); p.put("command",command); return p; }
    private static Integer nonnegative(int n) { return n < 0 ? null : Integer.valueOf(n); }
    private static String text(String s) { return s == null ? "" : s; }
    private static String emptyToNull(String s) { return s == null || s.isEmpty() ? null : s; }
    private static void emit(String kind, Map<String,Object> payload) {
        // Allocate before queueing/encoding. A dropped event leaves a detectable sequence gap.
        long seq = ++sequence;
        Map<String,Object> e = new LinkedHashMap<String,Object>();
        e.put("api_version","2"); e.put("event_id",UUID.randomUUID().toString()); e.put("stream_id",stream); e.put("sequence",seq);
        e.put("occurred_at",Instant.now().toString()); e.put("source","ameise_client"); e.put("kind",kind);
        Map<String,Object> identity = new LinkedHashMap<String,Object>(); identity.put("game_id",game); identity.put("tournament_id",tournament); identity.put("model_instance_id",model);
        e.put("game",identity); e.put("turn_id",turn); e.put("payload",payload);
        String packet = json(e);
        if (packet.length() > 1024*1024 || !QUEUE.offer(packet)) { warn("Observation dropped (queue full or packet too large), sequence " + seq); return; }
        if (!workerStarted) {
            Thread worker = new Thread(new Runnable() { public void run() { publish(); } }, "ameise-observation-publisher");
            worker.setDaemon(true); worker.start(); workerStarted = true;
        }
    }
    private static void publish() {
        try {
            URL url = new URL(System.getProperty("ameise.observer.url", "http://127.0.0.1:8765/api/v2/events"));
            if (!"http".equals(url.getProtocol()) || url.getUserInfo() != null) throw new IOException("Expected HTTP observation URL");
            while (true) {
                String packet = QUEUE.take(); byte[] bytes = packet.getBytes(StandardCharsets.UTF_8);
                int delay = 250;
                while (true) {
                    HttpURLConnection connection = null;
                    try {
                        connection = (HttpURLConnection) url.openConnection(Proxy.NO_PROXY);
                        connection.setInstanceFollowRedirects(false); connection.setConnectTimeout(1000); connection.setReadTimeout(1500);
                        connection.setRequestMethod("POST"); connection.setDoOutput(true); connection.setRequestProperty("Content-Type","application/json"); connection.setFixedLengthStreamingMode(bytes.length);
                        try (OutputStream out = connection.getOutputStream()) { out.write(bytes); }
                        int status = connection.getResponseCode();
                        if (status == 200 || status == 201) break;
                        if (status >= 400 && status < 500) { warn("Observation rejected by receiver, HTTP " + status); break; }
                        throw new IOException("Receiver unavailable");
                    } catch (IOException e) {
                        if (delay == 250) warn("Receiver unavailable; retrying the same observation in background");
                        Thread.sleep(delay); delay = Math.min(delay * 2, 5000);
                    } finally { if (connection != null) connection.disconnect(); }
                }
            }
        } catch (Throwable e) { warn("Observation publisher stopped; simulator transport remains independent"); }
    }
    static String json(Object value) {
        if (value == null) return "null";
        if (value instanceof Number || value instanceof Boolean) return value.toString();
        if (value instanceof Map) {
            StringBuilder b = new StringBuilder("{"); boolean first = true;
            for (Object entry : ((Map)value).entrySet()) { Map.Entry e = (Map.Entry)entry; if (!first) b.append(','); first=false; b.append(json(e.getKey().toString())).append(':').append(json(e.getValue())); }
            return b.append('}').toString();
        }
        if (value instanceof Iterable) {
            StringBuilder b = new StringBuilder("["); boolean first=true;
            for (Object item : (Iterable)value) { if (!first) b.append(','); first=false; b.append(json(item)); }
            return b.append(']').toString();
        }
        StringBuilder b=new StringBuilder("\"");
        for (char c : value.toString().toCharArray()) {
            if (c=='"' || c=='\\') b.append('\\').append(c);
            else if (c < 32 || Character.isSurrogate(c)) b.append(String.format("\\u%04x",(int)c));
            else b.append(c);
        }
        return b.append('"').toString();
    }
    private static void warn(String message) { System.err.println("[AMEISE observer] " + message); }
}
