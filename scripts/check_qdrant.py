from qdrant_client import QdrantClient

client = QdrantClient(url="http://localhost:6333")
collections = client.get_collections().collections
for c in collections:
    status = client.get_collection(collection_name=c.name)
    print(f"{c.name}: {status.points_count} points")
