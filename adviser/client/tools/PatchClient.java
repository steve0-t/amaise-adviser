import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;
import jdk.internal.org.objectweb.asm.*;

/** JDK 8 build tool only. Original methods and DTO class bytes are retained. */
public final class PatchClient implements Opcodes {
    static final String HOOK = "ameise/observer/ObservationHook";
    static int hooks;
    static byte[] patch(byte[] bytes, final boolean sending) {
        ClassReader reader = new ClassReader(bytes);
        ClassWriter writer = new ClassWriter(reader,ClassWriter.COMPUTE_MAXS);
        reader.accept(new ClassVisitor(ASM5,writer) {
            public MethodVisitor visitMethod(int access, final String name, final String desc, String signature, String[] exceptions) {
                MethodVisitor base = super.visitMethod(access,name,desc,signature,exceptions);
                return new MethodVisitor(ASM5,base) {
                    public void visitCode() {
                        super.visitCode();
                        if (!sending && (name.equals("exitApplication") || name.equals("informOnPollingFailure"))) { super.visitMethodInsn(INVOKESTATIC,HOOK,"failed","()V",false); hooks++; }
                    }
                    public void visitMethodInsn(int opcode,String owner,String method,String descriptor,boolean itf) {
                        if (sending && name.equals("executeRequest") && owner.equals("java/io/ObjectOutputStream") && method.equals("writeObject")) {
                            super.visitInsn(DUP); super.visitMethodInsn(INVOKESTATIC,HOOK,"before","(Ljava/lang/Object;)V",false); hooks++;
                        }
                        super.visitMethodInsn(opcode,owner,method,descriptor,itf);
                        if (sending && name.equals("executeRequest") && owner.equals("java/io/ObjectOutputStream") && method.equals("flush")) {
                            super.visitMethodInsn(INVOKESTATIC,HOOK,"sent","()V",false); hooks++;
                        }
                        if (!sending && name.equals("getObject") && method.equals("processObject") && descriptor.equals("(Lameise/shared/communication/MessageObject;)Z")) {
                            super.visitInsn(DUP); super.visitVarInsn(ALOAD,0); super.visitVarInsn(ALOAD,1);
                            super.visitMethodInsn(INVOKESTATIC,HOOK,"processed","(ZLameise/client/LogicalUnit;Lameise/shared/communication/MessageObject;)V",false); hooks++;
                        }
                    }
                    public void visitTypeInsn(int opcode,String type) {
                        if (sending && name.equals("executeRequest") && opcode==NEW && type.equals("ameise/client/exceptions/SendingFailedException")) { super.visitMethodInsn(INVOKESTATIC,HOOK,"failed","()V",false); hooks++; }
                        super.visitTypeInsn(opcode,type);
                    }
                };
            }
        },0);
        return writer.toByteArray();
    }
    static byte[] read(InputStream in) throws IOException { ByteArrayOutputStream out=new ByteArrayOutputStream(); byte[] b=new byte[8192]; int n; while((n=in.read(b))!=-1)out.write(b,0,n);return out.toByteArray(); }
    public static void main(String[] args) throws Exception {
        Path original=Paths.get(args[0]), output=Paths.get(args[1]), classes=Paths.get(args[2]);
        if (original.toRealPath().equals(output.toAbsolutePath().normalize())) throw new IllegalArgumentException("Output must be a copy");
        try (JarFile input=new JarFile(original.toFile()); JarOutputStream out=new JarOutputStream(Files.newOutputStream(output))) {
            Enumeration<JarEntry> entries=input.entries();
            while(entries.hasMoreElements()) {
                JarEntry e=entries.nextElement(); String name=e.getName();
                if (name.startsWith("META-INF/") && (name.endsWith(".SF") || name.endsWith(".RSA") || name.endsWith(".DSA"))) throw new IllegalStateException("Signed JAR requires explicit signing workflow");
                byte[] bytes=read(input.getInputStream(e));
                if(name.equals("ameise/client/SendingUnit.class"))bytes=patch(bytes,true);
                if(name.equals("ameise/client/LogicalUnit.class"))bytes=patch(bytes,false);
                JarEntry copy=new JarEntry(name); copy.setTime(e.getTime()); out.putNextEntry(copy);out.write(bytes);out.closeEntry();
            }
            try (java.util.stream.Stream<Path> paths=Files.walk(classes.resolve("ameise/observer"))) {
                for(Path p:(Iterable<Path>)paths.filter(Files::isRegularFile)::iterator) {
                    out.putNextEntry(new JarEntry(classes.relativize(p).toString().replace(File.separatorChar,'/')));out.write(Files.readAllBytes(p));out.closeEntry();
                }
            }
        }
        if(hooks!=6)throw new IllegalStateException("Unexpected original bytecode: installed "+hooks+" hooks instead of 6");
        // Check every original entry, including manifest, resources and serialized DTOs.
        try(JarFile a=new JarFile(original.toFile());JarFile b=new JarFile(output.toFile())) {
            Enumeration<JarEntry> entries=a.entries();
            while(entries.hasMoreElements()) {
                JarEntry e=entries.nextElement(); String n=e.getName();
                if(n.equals("ameise/client/SendingUnit.class") || n.equals("ameise/client/LogicalUnit.class"))continue;
                if(!Arrays.equals(read(a.getInputStream(e)),read(b.getInputStream(b.getJarEntry(n)))))throw new IllegalStateException("Changed original entry: "+n);
            }
        }
        System.out.println("Verified: 2 patched client classes; all original protocol classes, manifest and resources byte-identical.");
    }
}
