import com.healthmarketscience.jackcess.*;
import java.io.File;
public class TextKeysFixture {
  public static void main(String[] a) throws Exception {
    File out = new File(a[0]); out.delete();
    try (Database d = new DatabaseBuilder(out).setFileFormat(Database.FileFormat.V2010).create()) {
      d.setEnforceForeignKeys(false);
      Table p = new TableBuilder("Codes").addColumn(new ColumnBuilder("Code", DataType.TEXT)).addColumn(new ColumnBuilder("Label", DataType.TEXT)).setPrimaryKey("Code").toTable(d);
      Table c = new TableBuilder("Items").addColumn(new ColumnBuilder("ID", DataType.LONG)).addColumn(new ColumnBuilder("Code", DataType.TEXT)).setPrimaryKey("ID").toTable(d);
      new RelationshipBuilder(p, c).addColumns("Code", "Code").setReferentialIntegrity().toRelationship(d);
      p.addRow("ab", "lower"); p.addRow("CD", "upper");
      c.addRow(1, "AB"); c.addRow(2, "cd"); c.addRow(3, "ab");
    }
  }
}
