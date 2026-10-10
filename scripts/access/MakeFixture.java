// Builds the binary Access test fixtures (tests/fixtures/access/orders.{accdb,mdb}.gz)
// with Jackcess (Apache-2.0), which writes Jet 4 and ACE files:
//
//   javac -cp jackcess.jar:commons-lang3.jar:commons-logging.jar MakeFixture.java
//   java -cp .:jackcess.jar:commons-lang3.jar:commons-logging.jar MakeFixture out.accdb V2010
//   gzip -9 -n out.accdb
//
// Jackcess cannot create saved queries, so the queries are written as the
// MSysObjects and MSysQueries rows Access itself stores (docs/access-format.md §4.10).
import com.healthmarketscience.jackcess.*;
import java.io.File;
import java.math.BigDecimal;
import java.time.LocalDateTime;
import java.util.*;

public class MakeFixture {
  public static void main(String[] a) throws Exception {
    Database.FileFormat format = Database.FileFormat.valueOf(a[1]);
    Database db = new DatabaseBuilder(new File(a[0])).setFileFormat(format).create();
    db.setEvaluateExpressions(false);

    Table customers = new TableBuilder("Customers")
        .addColumn(new ColumnBuilder("ID", DataType.LONG).setAutoNumber(true))
        .addColumn(new ColumnBuilder("Company", DataType.TEXT).setLengthInUnits(100)
            .putProperty("Required", true).putProperty("Caption", "Company name"))
        .addColumn(new ColumnBuilder("Category", DataType.TEXT).setLengthInUnits(50)
            .putProperty("DisplayControl", DataType.INT, (short) 111)
            .putProperty("RowSourceType", "Value List")
            .putProperty("RowSource", "\"Retail\";\"Wholesale\";\"Online\""))
        .addColumn(new ColumnBuilder("Notes", DataType.MEMO))
        .addColumn(new ColumnBuilder("Website", DataType.MEMO).setHyperlink(true))
        .addColumn(new ColumnBuilder("Active", DataType.BOOLEAN).putProperty("DefaultValue", "Yes"))
        .addIndex(new IndexBuilder(IndexBuilder.PRIMARY_KEY_NAME).addColumns("ID").setPrimaryKey())
        .addIndex(new IndexBuilder("Company").addColumns("Company").setUnique())
        .toTable(db);

    Table orders = new TableBuilder("Orders")
        .addColumn(new ColumnBuilder("ID", DataType.LONG).setAutoNumber(true))
        .addColumn(new ColumnBuilder("Customer", DataType.LONG)
            .putProperty("DisplayControl", DataType.INT, (short) 111)
            .putProperty("RowSourceType", "Table/Query")
            .putProperty("RowSource", "SELECT Customers.ID, Customers.Company FROM Customers ORDER BY Customers.Company;")
            .putProperty("BoundColumn", DataType.INT, (short) 1)
            .putProperty("ColumnCount", DataType.INT, (short) 2)
            .putProperty("ColumnWidths", "0;1440"))
        .addColumn(new ColumnBuilder("Order Date", DataType.SHORT_DATE_TIME)
            .putProperty("Format", "Short Date").putProperty("DefaultValue", "=Date()"))
        .addColumn(new ColumnBuilder("Amount", DataType.MONEY)
            .putProperty("ValidationRule", ">=0").putProperty("ValidationText", "Amount cannot be negative."))
        .addColumn(new ColumnBuilder("Quantity", DataType.INT))
        .addColumn(new ColumnBuilder("Discount", DataType.NUMERIC).setPrecision(5).setScale(2))
        .addColumn(new ColumnBuilder("Paid", DataType.BOOLEAN))
        .addColumn(new ColumnBuilder("Shipped", DataType.SHORT_DATE_TIME))
        .addIndex(new IndexBuilder(IndexBuilder.PRIMARY_KEY_NAME).addColumns("ID").setPrimaryKey())
        .addIndex(new IndexBuilder("Order Date").addColumns("Order Date"))
        .putProperty("ValidationRule", "[Shipped] Is Null Or [Shipped]>=[Order Date]")
        .putProperty("ValidationText", "An order ships after it is placed.")
        .toTable(db);

    new RelationshipBuilder(customers, orders).addColumns("ID", "Customer")
        .setReferentialIntegrity().setCascadeDeletes().setName("CustomersOrders").toRelationship(db);

    StringBuilder longNote = new StringBuilder();
    for (int i = 0; i < 400; i++) longNote.append("Line ").append(i).append(": a long memo that spans several pages. ");
    customers.addRow(Column.AUTO_NUMBER, "Contoso Ltd", "Retail", longNote.toString(), "Contoso#https://contoso.example/#", true);
    customers.addRow(Column.AUTO_NUMBER, "Fabrikam, Inc.", "Wholesale", "Bulk buyer", null, true);
    customers.addRow(Column.AUTO_NUMBER, "Northwind Café", "Online", "Ünïcödé — ok", "#https://northwind.example/#", false);
    customers.addRow(Column.AUTO_NUMBER, "Adventure Works", null, null, null, true);
    customers.addRow(Column.AUTO_NUMBER, "Tailspin Toys", "Retail", "", null, true);

    Object[][] rows = {
      {1, "2024-01-05T00:00", "120.50", 3, "0.10", true, "2024-01-07T10:30"},
      {1, "2024-02-11T00:00", "75.00", 1, "0.00", true, "2024-02-12T09:00"},
      {2, "2024-02-14T00:00", "1999.99", 40, "0.15", false, null},
      {2, "2024-03-01T00:00", "250.00", 10, "0.05", true, "2024-03-03T16:45"},
      {3, "2024-03-09T00:00", "18.25", 2, "0.00", false, null},
      {4, "2024-03-15T00:00", "640.00", 16, "0.12", true, "2024-03-20T08:15"},
      {5, "2024-04-02T00:00", "89.90", 3, "0.00", true, "2024-04-02T17:00"},
      {5, "2024-04-18T00:00", "4500.00", 90, "0.20", false, null},
    };
    for (Object[] r : rows) {
      orders.addRow(Column.AUTO_NUMBER, r[0], LocalDateTime.parse((String) r[1]), new BigDecimal((String) r[2]), ((Integer) r[3]).shortValue(),
          new BigDecimal((String) r[4]), r[5], r[6] == null ? null : LocalDateTime.parse((String) r[6]));
    }

    // Saved queries, as MSysQueries rows: (attribute, expression, flag, name1, name2).
    query(db, "Customer Totals", 0, new Object[][] {
      {1, null, 1, null, null},
      {3, null, 0, null, null},
      {5, null, null, "Customers", null},
      {5, null, null, "Orders", null},
      {6, "Customers.Company", null, null, null},
      {6, "Sum(Orders.Amount)", null, "Total", null},
      {6, "Count(*)", null, "Order Count", null},
      {7, "Customers.ID = Orders.Customer", 1, "Customers", "Orders"},
      {9, "Customers.Company", null, null, null},
      {11, "Sum(Orders.Amount)", null, "D", null},
    });
    query(db, "Big Orders", 0, new Object[][] {
      {1, null, 1, null, null},
      {2, null, 5, "Minimum amount", null},
      {3, null, 1, null, null},
      {5, null, null, "Orders", null},
      {8, "Orders.Amount>=[Minimum amount] And Orders.[Order Date]>=#1/1/2024#", null, null, null},
    });
    query(db, "Unpaid Orders", 0, new Object[][] {
      {1, null, 1, null, null},
      {3, null, 0, null, null},
      {5, null, null, "Customer Totals", null},
      {5, null, null, "Orders", null},
      {6, "Orders.ID", null, null, null},
      {6, "[Customer Totals].Company", null, null, null},
      {6, "IIf([Paid],\"Paid\",\"Due \" & Format(Orders.Amount,\"Currency\"))", null, "Status", null},
      {7, "[Customer Totals].Company = DLookup(\"Company\",\"Customers\",\"ID=\" & Orders.Customer)", 1, "Customer Totals", "Orders"},
      {8, "Not Orders.Paid", null, null, null},
    });
    query(db, "Delete Old Orders", 32, new Object[][] {
      {1, null, 5, null, null},
      {5, null, null, "Orders", null},
      {6, "Orders.*", null, null, null},
      {8, "Orders.[Order Date]<#1/1/2020#", null, null, null},
    });
    db.close();
  }

  static int nextId = 0x7FFF0000;

  static void query(Database db, String name, int flags, Object[][] rows) throws Exception {
    Table objects = db.getSystemTable("MSysObjects");
    int id = nextId++;
    Map<String, Object> o = new HashMap<>();
    o.put("Id", id);
    o.put("ParentId", 0x0F000000);
    o.put("Name", name);
    o.put("Type", (short) 5);
    o.put("Flags", flags);
    o.put("DateCreate", LocalDateTime.parse("2024-01-01T00:00"));
    o.put("DateUpdate", LocalDateTime.parse("2024-01-01T00:00"));
    objects.addRowFromMap(o);
    Table q = db.getSystemTable("MSysQueries");
    List<Object[]> all = new ArrayList<>();
    all.add(new Object[] {0, null, null, null, null});
    all.addAll(Arrays.asList(rows));
    all.add(new Object[] {255, null, null, null, null});
    int order = 0;
    for (Object[] r : all) {
      Map<String, Object> m = new HashMap<>();
      m.put("Attribute", (byte) ((Integer) r[0]).intValue());
      m.put("Expression", r[1]);
      m.put("Flag", r[2] == null ? null : ((Integer) r[2]).shortValue());
      m.put("Name1", r[3]);
      m.put("Name2", r[4]);
      m.put("ObjectId", id);
      m.put("Order", new byte[] {0, 0, 0, (byte) order++});
      q.addRowFromMap(m);
    }
  }
}
