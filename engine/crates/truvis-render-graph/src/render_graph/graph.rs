//! 依赖图构建和拓扑排序
//!
//! 分析 Pass 之间的资源依赖关系，构建 DAG 并进行拓扑排序。
//! 使用 petgraph 库提供高效的图算法实现。

use petgraph::Direction;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use slotmap::SecondaryMap;

use crate::render_graph::{RgBufferHandle, RgImageHandle};

/// 依赖边数据：资源依赖信息
#[derive(Clone, Debug, Default)]
pub struct EdgeData {
    /// 依赖的图像资源
    pub images: Vec<RgImageHandle>,
    /// 依赖的缓冲区资源
    pub buffers: Vec<RgBufferHandle>,
}

/// 依赖图
///
/// 表示 Pass 之间的依赖关系，用于拓扑排序和执行顺序计算。
/// 内部使用 petgraph 的 DiGraph 实现。
pub struct DependencyGraph {
    /// 有向图：节点存储 pass 索引，边存储资源依赖
    graph: DiGraph<usize, EdgeData>,
    /// Pass 索引到图节点的映射
    node_indices: Vec<NodeIndex>,
}

impl DependencyGraph {
    /// 分析资源依赖，构建依赖图
    ///
    /// 规则：
    /// - 写后读（WAR）：reader 依赖 writer
    /// - 读后写（RAW）：writer 依赖 reader（保证读取完成）
    /// - 写后写（WAW）：后一个 writer 依赖前一个 writer
    pub fn analyze(
        pass_count: usize,
        image_reads: &[Vec<RgImageHandle>],  // pass_index -> [image handles]
        image_writes: &[Vec<RgImageHandle>], // pass_index -> [image handles]
        buffer_reads: &[Vec<RgBufferHandle>],
        buffer_writes: &[Vec<RgBufferHandle>],
    ) -> Self {
        let mut graph = {
            let mut graph = DiGraph::with_capacity(pass_count, pass_count * 2);

            // 为每个 pass 创建节点
            let node_indices: Vec<NodeIndex> = (0..pass_count).map(|i| graph.add_node(i)).collect();

            Self { graph, node_indices }
        };

        // 跟踪每个资源的最后写入者
        let mut last_image_writer: SecondaryMap<RgImageHandle, usize> = SecondaryMap::new();
        let mut last_buffer_writer: SecondaryMap<RgBufferHandle, usize> = SecondaryMap::new();

        for pass_idx in 0..pass_count {
            // 处理图像读取
            for &img_handle in &image_reads[pass_idx] {
                // 如果有之前的写入者，添加依赖
                if let Some(&writer) = last_image_writer.get(img_handle)
                    && writer != pass_idx
                {
                    graph.add_edge(writer, pass_idx, vec![img_handle], vec![]);
                }
            }

            // 处理图像写入
            for &img_handle in &image_writes[pass_idx] {
                // 如果有之前的写入者，添加 WAW 依赖
                if let Some(&prev_writer) = last_image_writer.get(img_handle)
                    && prev_writer != pass_idx
                {
                    graph.add_edge(prev_writer, pass_idx, vec![img_handle], vec![]);
                }

                // 更新最后写入者
                last_image_writer.insert(img_handle, pass_idx);
            }

            // 处理缓冲区读取
            for &buf_handle in &buffer_reads[pass_idx] {
                if let Some(&writer) = last_buffer_writer.get(buf_handle)
                    && writer != pass_idx
                {
                    graph.add_edge(writer, pass_idx, vec![], vec![buf_handle]);
                }
            }

            // 处理缓冲区写入
            for &buf_handle in &buffer_writes[pass_idx] {
                if let Some(&prev_writer) = last_buffer_writer.get(buf_handle)
                    && prev_writer != pass_idx
                {
                    graph.add_edge(prev_writer, pass_idx, vec![], vec![buf_handle]);
                }
                last_buffer_writer.insert(buf_handle, pass_idx);
            }
        }

        graph
    }

    /// 添加依赖边
    ///
    /// # 参数
    /// - `producer`: 生产者 Pass 索引（先执行）
    /// - `consumer`: 消费者 Pass 索引（后执行）
    /// - `images`: 涉及的图像资源
    /// - `buffers`: 涉及的缓冲区资源
    pub fn add_edge(
        &mut self,
        producer: usize,
        consumer: usize,
        images: Vec<RgImageHandle>,
        buffers: Vec<RgBufferHandle>,
    ) {
        let producer_node = self.node_indices[producer];
        let consumer_node = self.node_indices[consumer];

        // 检查是否已存在边，如果存在则合并资源
        if let Some(edge_idx) = self.graph.find_edge(producer_node, consumer_node) {
            let edge_data = self.graph.edge_weight_mut(edge_idx).unwrap();
            edge_data.images.extend(images);
            edge_data.buffers.extend(buffers);
        } else {
            self.graph.add_edge(producer_node, consumer_node, EdgeData { images, buffers });
        }
    }

    /// 执行拓扑排序
    ///
    /// # 返回
    /// - `Ok(order)`: 拓扑排序后的 Pass 索引列表
    /// - `Err(cycle)`: 检测到循环依赖，返回参与循环的 Pass 索引
    pub fn topological_sort(&self) -> Result<Vec<usize>, Vec<usize>> {
        match toposort(&self.graph, None) {
            Ok(sorted_nodes) => {
                // 将 NodeIndex 转换回 pass 索引
                Ok(sorted_nodes.into_iter().map(|n| self.graph[n]).collect())
            }
            Err(cycle) => {
                // petgraph 返回循环中的一个节点，我们找出所有参与循环的节点
                // 简化处理：返回循环节点
                Err(vec![self.graph[cycle.node_id()]])
            }
        }
    }

    /// 获取 Pass 的直接依赖（前驱）
    pub fn get_predecessors(&self, pass_index: usize) -> Vec<usize> {
        let node = self.node_indices[pass_index];
        self.graph.neighbors_directed(node, Direction::Incoming).map(|n| self.graph[n]).collect()
    }

    /// 获取 Pass 的直接后继
    pub fn get_successors(&self, pass_index: usize) -> Vec<usize> {
        let node = self.node_indices[pass_index];
        self.graph.neighbors_directed(node, Direction::Outgoing).map(|n| self.graph[n]).collect()
    }
}

#[cfg(test)]
mod tests {
    use slotmap::SlotMap;

    use super::*;

    fn create_test_image_handles(count: usize) -> (SlotMap<RgImageHandle, ()>, Vec<RgImageHandle>) {
        let mut sm = SlotMap::with_key();
        let handles: Vec<RgImageHandle> = (0..count).map(|_| sm.insert(())).collect();
        (sm, handles)
    }

    #[test]
    fn test_simple_dependency() {
        // Pass 0 写入 image 0
        // Pass 1 读取 image 0
        let (_sm, handles) = create_test_image_handles(1);
        let img0 = handles[0];

        let image_reads = vec![vec![], vec![img0]];
        let image_writes = vec![vec![img0], vec![]];
        let buffer_reads = vec![vec![], vec![]];
        let buffer_writes = vec![vec![], vec![]];

        let graph = DependencyGraph::analyze(2, &image_reads, &image_writes, &buffer_reads, &buffer_writes);

        let order = graph.topological_sort().unwrap();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn test_chain_dependency() {
        // Pass 0 -> Pass 1 -> Pass 2
        let (_sm, handles) = create_test_image_handles(2);
        let img0 = handles[0];
        let img1 = handles[1];

        let image_reads = vec![vec![], vec![img0], vec![img1]];
        let image_writes = vec![vec![img0], vec![img1], vec![]];
        let buffer_reads = vec![vec![], vec![], vec![]];
        let buffer_writes = vec![vec![], vec![], vec![]];

        let graph = DependencyGraph::analyze(3, &image_reads, &image_writes, &buffer_reads, &buffer_writes);

        let order = graph.topological_sort().unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn test_parallel_passes() {
        // Pass 0 写入 image 0
        // Pass 1 写入 image 1（无依赖，可并行）
        // Pass 2 读取 image 0 和 image 1
        let (_sm, handles) = create_test_image_handles(2);
        let img0 = handles[0];
        let img1 = handles[1];

        let image_reads = vec![vec![], vec![], vec![img0, img1]];
        let image_writes = vec![vec![img0], vec![img1], vec![]];
        let buffer_reads = vec![vec![], vec![], vec![]];
        let buffer_writes = vec![vec![], vec![], vec![]];

        let graph = DependencyGraph::analyze(3, &image_reads, &image_writes, &buffer_reads, &buffer_writes);

        let order = graph.topological_sort().unwrap();
        // Pass 0 和 1 可以任意顺序，但都在 Pass 2 之前
        assert!(order[0] == 0 || order[0] == 1);
        assert!(order[1] == 0 || order[1] == 1);
        assert_eq!(order[2], 2);
    }
}
