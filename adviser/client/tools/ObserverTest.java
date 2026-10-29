import ameise.client.LogicalUnit;
import ameise.client.util.CurrentModels;
import ameise.observer.ObservationHook;
import ameise.shared.communication.*;
import ameise.shared.communication.content.*;
import com.sun.net.httpserver.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;
import java.util.regex.*;

/** Synthetic protocol fixtures: never logs in or contacts AMEISE. */
public final class ObserverTest {
    static void check(boolean condition,String message) { if(!condition)throw new AssertionError(message); }
    static String field(String packet,String key) { Matcher m=Pattern.compile("\""+key+"\":\"([^\"]*)\"").matcher(packet);check(m.find(),"Missing "+key);return m.group(1); }
    static MessageObject command(String text, boolean restart) {
        CommandObject c=new CommandObject();c.setGameId(17);c.setModelInstanceId(9);c.setSpielstandId(42);c.setCommandString(text);c.setRestart(restart);return new MessageObject(2,new RequestManagerObject(2,c));
    }
    static void result(LogicalUnit l) {
        CommandResultObject r=new CommandResultObject();r.setSpielstandId(43);r.setCommandString("rewritten upstream text");r.setFeedback("@Wwarning\u001e@Eerror");r.setKernelResultString("@Praw output");r.setKernelDatString("02/01/2000");ObservationHook.processed(true,l,new MessageObject(3,new ResultObject(2,r)));
    }
    public static void main(String[] args) throws Exception {
        boolean disabled=args.length>0 && args[0].equals("disabled");
        Class.forName("ameise.client.SendingUnit");Class.forName("ameise.client.LogicalUnit");
        final List<String> received=Collections.synchronizedList(new ArrayList<String>());
        final List<String> attempts=Collections.synchronizedList(new ArrayList<String>());
        HttpServer receiver=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
        receiver.createContext("/api/v2/events",new HttpHandler(){public void handle(HttpExchange e)throws IOException {
            ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] b=new byte[4096];int n;while((n=e.getRequestBody().read(b))!=-1)out.write(b,0,n);
            String body=new String(out.toByteArray(),StandardCharsets.UTF_8);attempts.add(body);
            int status=attempts.size()<=2?503:201;if(status==201)received.add(body);
            e.sendResponseHeaders(status,0);e.getResponseBody().close();e.close();
        }});
        receiver.start();
        System.setProperty("ameise.observer.enabled",disabled?"false":"true");System.setProperty("ameise.observer.url","http://127.0.0.1:"+receiver.getAddress().getPort()+"/api/v2/events");
        try {
            LogicalUnit l=new LogicalUnit(null,null);
            GameData game=new GameData(17,"","",123,42,555,"02/01/2000");
            TournamentData tournament=new TournamentData();tournament.setTournamentId(4);tournament.setModelInstanceId(9);
            Vector row=new Vector();row.add(game);row.add(tournament);Vector rows=new Vector();rows.add(row);
            java.lang.reflect.Field models=LogicalUnit.class.getDeclaredField("modelsAndGames");models.setAccessible(true);models.set(l,new CurrentModels(rows));
            TurnData turn=new TurnData();turn.setTurnId(42);turn.setDate("02/01/2000");
            ModelInstanceData model=new ModelInstanceData();model.setModelInstanceId(9);model.setName("Fixture game");Vector modelVector=new Vector();modelVector.add(model);
            Vector selection=new Vector();selection.add(game);selection.add(turn);selection.add(new Vector());selection.add(modelVector);
            long start=System.nanoTime();
            ObservationHook.processed(true,l,new MessageObject(1,new DBResultObject(6,true,selection)));
            MessageObject first=command("proceed 1",true);ObservationHook.before(first);ObservationHook.sent();
            ((CommandObject)((RequestManagerObject)first.getContent()).getContent()).setCommandString("MUTATED AFTER SEND");
            ObservationHook.processed(true,l,new MessageObject(3,new ResultObject(3,new TimeObject())));
            CallbackResultObject cb=new CallbackResultObject();cb.setSpielstandId(42);cb.setQuestionString("Choose a developer");cb.setAlternativesString("\\[12\\] Bernd, \\[27\\] Stefanie,");Vector answers=new Vector();answers.add(Integer.valueOf(12));answers.add(Integer.valueOf(27));cb.setAnswerVector(answers);
            ObservationHook.processed(true,l,new MessageObject(3,new ResultObject(1,cb)));
            ObservationHook.before(new MessageObject(2,new RequestManagerObject(1,new CallbackObject())));ObservationHook.sent();
            result(l);
            ObservationHook.before(command("proceed 1",false));ObservationHook.sent();ObservationHook.failed();result(l);
            ObservationHook.before(command("show me specification",true));ObservationHook.sent();result(l);
            long millis=TimeUnit.NANOSECONDS.toMillis(System.nanoTime()-start);check(millis<500,"HTTP must not block simulator thread: "+millis);
            if(disabled) {Thread.sleep(150);check(attempts.isEmpty(),"Disabled observer published packets");System.out.println("Disabled observer: no HTTP traffic; patched classes verified");return;}
            long deadline=System.currentTimeMillis()+8000;while(received.size()<10 && System.currentTimeMillis()<deadline)Thread.sleep(20);
            check(received.size()==10,"Expected 10 observations, got "+received.size());
            check(attempts.get(0).equals(attempts.get(1)) && attempts.get(0).equals(attempts.get(2)),"Retries must preserve packet identity");
            check(field(received.get(1),"request_id").equals(field(received.get(2),"request_id")),"Callback correlation");
            check(field(received.get(1),"request_id").equals(field(received.get(3),"request_id")),"Terminal result correlation");
            check(!field(received.get(1),"request_id").equals(field(received.get(4),"request_id")),"Next wire command needs new request ID");
            check(field(received.get(1),"command").equals("proceed 1") && field(received.get(3),"command").equals("proceed 1"),"Original sent command is immutable");
            check(received.get(2).contains("\"previous_answer_ids\":[12,27]") && received.get(2).contains("\"choices\":[]"),"Preserve real callback IDs and unparsed alternatives");
            check(received.get(0).contains("\"tournament_id\":4"),"Use tournament identity, not round ID");
            check(!field(received.get(0),"stream_id").equals(field(received.get(7),"stream_id")),"Restart needs new stream");
            for(String packet:received) {check(!packet.contains("person") && !packet.contains("MUTATED"),"Redacted immutable packets");}
            if(args.length>0) {try(PrintWriter output=new PrintWriter(args[0],"UTF-8")){for(String packet:received)output.println(packet);}}
            System.out.println("Observer fixtures passed: retry identity, callbacks, wait hints, macro steps, uncertainty, branch streams, disabled mode and nonblocking publication");
        } finally {receiver.stop(0);}
    }
}
